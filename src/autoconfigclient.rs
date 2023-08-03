use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::pin::{self, Pin};
use std::sync::Arc;

use crate::credential::{ClientSideId, LaunchDarklyCredential, RelayAutoConfigKey};
use crate::message_event_source::MessageParseError;
use crate::messages::{
    DeleteEvent, EnvironmentConfig, EnvironmentKey, Message, PatchEvent, ProjectKey, PutData,
    PutEvent,
};

use crate::eventsource::codec::Event;
use crate::eventsource::{EventSource, EventSourceError};
use backoff::backoff::Backoff;
use backoff::ExponentialBackoff;
use futures::{pin_mut, Future, Stream, TryStream};
use tokio::sync::oneshot;

use miette::Diagnostic;
use pin_project::pin_project;
use reqwest::{Client, Request, RequestBuilder};
use serde::Serialize;
use thiserror::Error;
use tracing::{
    debug, debug_span, error, error_span, instrument, span, trace, trace_span, warn, warn_span,
};

#[derive(Debug, Error, Diagnostic)]
pub enum AutoConfigClientError {
    #[error("unrecoverable error in event source stream")]
    EventSourceError(#[from] EventSourceError),
    #[error("error parsing autoconfig event")]
    EventParseError(#[from] MessageParseError),
}

#[pin_project]
pub struct AutoConfigClient {
    environments: HashMap<ClientSideId, EnvironmentConfig>,
    request_builder: RequestBuilder,
    #[pin]
    event_source: Pin<Box<EventSource<ExponentialBackoff>>>,
    changes: VecDeque<ConfigChangeEvent>,
    is_initialized: bool,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum ConfigChangeEvent {
    Initialized,
    Insert(EnvironmentConfig),
    // previous, current
    Update {
        previous: EnvironmentConfig,
        current: EnvironmentConfig,
    },
    Delete(EnvironmentConfig),
}

impl AutoConfigClient {
    #[instrument(skip(request_builder))]
    pub fn with_request_builder(
        credential: RelayAutoConfigKey,
        request_builder: RequestBuilder,
    ) -> Self {
        let builder = request_builder.header("authorization", credential.as_str());

        let event_source = Self::create_event_source(builder.try_clone().unwrap()).unwrap();
        Self {
            request_builder: builder,
            environments: HashMap::new(),
            event_source: Box::pin(event_source),
            changes: VecDeque::new(),
            is_initialized: false,
        }
    }

    #[instrument(skip(client, credential), fields(credential=%credential))]
    pub fn new(credential: RelayAutoConfigKey, client: Client) -> Self {
        let url = "https://stream.launchdarkly.com/relay_auto_config";
        debug!(url, "created autoconfig client");
        let builder = client.get(url).header("authorization", credential.as_str());
        let event_source = Self::create_event_source(builder.try_clone().unwrap()).unwrap();
        Self {
            request_builder: builder,
            environments: HashMap::new(),
            event_source: Box::pin(event_source),
            changes: VecDeque::new(),
            is_initialized: false,
        }
    }
    #[instrument(skip(self), fields(environment_count=self.environments.len()))]
    pub fn environments(&self) -> &HashMap<ClientSideId, EnvironmentConfig> {
        &self.environments
    }
    #[instrument(skip(self))]
    pub fn by_project_key(
        &self,
        project_key: ProjectKey,
    ) -> impl Iterator<Item = &EnvironmentConfig> + '_ {
        self.environments()
            .values()
            .filter(move |env| env.proj_key == project_key)
    }

    #[instrument(skip(self))]
    pub fn get_environment(
        &self,
        project_key: ProjectKey,
        env_key: EnvironmentKey,
    ) -> Option<&EnvironmentConfig> {
        self.by_project_key(project_key)
            .find(move |env| env.env_key == env_key)
    }

    #[instrument(skip(self, environments))]
    pub fn replace_environments(&mut self, environments: HashMap<ClientSideId, EnvironmentConfig>) {
        debug!(
            environment_count = environments.len(),
            "replacing environments"
        );
        self.environments = environments;
    }
    fn generate_init_changes(&mut self) {
        for env in self.environments.values() {
            self.changes
                .push_back(ConfigChangeEvent::Insert(env.clone()));
        }
    }
    #[instrument(skip(self, environments))]
    pub fn load_environments(&mut self, environments: HashMap<ClientSideId, EnvironmentConfig>) {
        debug!(
            environment_count = environments.len(),
            "loading environments"
        );
        if self.environments.is_empty() {
            debug!("initialized in-memory-cache");
            self.environments = environments;
            return;
        }

        for (key, value) in environments {
            match self.environments.entry(key) {
                Entry::Occupied(mut entry) => {
                    let span = debug_span!("merge", env_id = %value.env_id, proj_key=%value.proj_key, env_key=%value.env_key, received_version=%value.version);
                    let _enter = span.enter();
                    let existing = entry.get_mut();
                    if existing.version < value.version {
                        debug!("updating environment");
                        *existing = value;
                    } else {
                        debug!("ignoring environment update");
                    }
                }
                Entry::Vacant(entry) => {
                    debug!("adding environment");
                    entry.insert(value);
                }
            }
        }
    }

    #[instrument(skip(self))]
    fn reconnect(mut self: std::pin::Pin<&mut Self>) {
        *self.as_mut().project().event_source =
            Box::pin(Self::create_event_source(self.request_builder.try_clone().unwrap()).unwrap());
    }

    #[instrument]
    fn create_event_source(
        request_builder: RequestBuilder,
    ) -> Result<EventSource<ExponentialBackoff>, EventSourceError> {
        EventSource::try_with_backoff(request_builder, None, ExponentialBackoff::default())
    }
    #[instrument(level= "debug", skip(source, value), fields(proj_key=%value.proj_key, env_key=%value.env_key, received_version=%value.version))]
    fn update_environment(
        source: &mut HashMap<ClientSideId, EnvironmentConfig>,
        env_id: ClientSideId,
        value: EnvironmentConfig,
    ) -> Option<ConfigChangeEvent> {
        debug_assert!(env_id == value.env_id);
        match source.entry(env_id) {
            Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                if existing.version < value.version {
                    debug!("updating environment");
                    let previous_value = entry.insert(value.clone());
                    Some(ConfigChangeEvent::Update {
                        previous: previous_value,
                        current: value,
                    })
                } else {
                    debug!("ignoring environment update");
                    None
                }
            }
            Entry::Vacant(entry) => {
                debug!("adding environment");
                entry.insert(value.clone());
                Some(ConfigChangeEvent::Insert(value))
            }
        }
    }
    #[instrument(skip(self, msg))]
    fn process_message(
        mut self: std::pin::Pin<&mut Self>,
        msg: Message,
    ) -> VecDeque<ConfigChangeEvent> {
        let this = self.as_mut().project();

        match msg {
            Message::Put(PutEvent {
                path,
                data: PutData { environments },
            }) if path == "/" => {
                let span = debug_span!("put", path=?path, environment_count=?environments.len());
                let _enter = span.enter();
                let changes = if this.environments.is_empty() {
                    debug!("initializing in-memory cache");

                    let is_initialized = *this.is_initialized;
                    let mut changes = if is_initialized {
                        VecDeque::with_capacity(environments.len())
                    } else {
                        let mut c = VecDeque::with_capacity(environments.len() + 1);
                        c.push_back(ConfigChangeEvent::Initialized);
                        c
                    };
                    *this.environments = environments;

                    changes.extend(
                        this.environments
                            .values()
                            .map(|env| ConfigChangeEvent::Insert(env.clone())),
                    );
                    if is_initialized {
                        *this.is_initialized = true;
                    }
                    changes
                } else {
                    trace!("merging environments into in-memory cache");
                    let mut changes = VecDeque::new();
                    for (key, value) in environments {
                        if let Some(change) =
                            Self::update_environment(this.environments, key, value)
                        {
                            changes.push_back(change);
                        }
                    }
                    changes
                };
                changes
            }
            Message::Put(PutEvent { path, .. }) => warn_span!("put", path=?path).in_scope(|| {
                warn!("unexpected path in event");
                VecDeque::new()
            }),
            Message::Patch(PatchEvent {
                env_id,
                environment,
            }) => {
                debug_span!("patch", env_id=env_id.as_str(), received_version=%environment.version)
                    .in_scope(|| {
                        let mut changes = VecDeque::new();
                        if let Some(change) =
                            Self::update_environment(this.environments, env_id, environment)
                        {
                            changes.push_back(change);
                        }
                        changes
                    })
            }
            Message::Delete(DeleteEvent { env_id, version }) => {
                debug_span!("delete", env_id=env_id.as_str(), received_version=%version).in_scope(
                    || {
                        let mut changes = VecDeque::new();
                        let entry = this.environments.entry(env_id.clone());
                        match entry {
                            Entry::Occupied(e) => {
                                debug_span!("occupied", previous_version=%e.get().version).in_scope(
                                    || {
                                        if e.get().version < version {
                                            debug!("removing environment with received version");
                                            changes
                                                .push_back(ConfigChangeEvent::Delete(e.remove()));
                                        } else {
                                            debug!("ignoring delete with older version");
                                        }
                                    },
                                )
                            }
                            Entry::Vacant(_) => {
                                debug_span!("vacant").in_scope(|| {
                                    debug!("received delete event for unknown environment");
                                });
                            }
                        }
                        changes
                    },
                )
            }
            Message::Reconnect => {
                let span = debug_span!("reconnect");
                let _span = span.enter();
                debug!("server requested reconnect");
                self.as_mut().reconnect();
                VecDeque::new()
            }
        }
    }
}

impl Stream for AutoConfigClient {
    type Item = Result<ConfigChangeEvent, AutoConfigClientError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let span = debug_span!("event");
        let _span = span.enter();
        loop {
            let this = self.as_mut().project();
            match this.changes.pop_front() {
                Some(change) => return std::task::Poll::Ready(Some(Ok(change))),
                None => match futures::ready!(this.event_source.poll_next(cx)) {
                    Some(Ok(event)) => {
                        let msg = Message::try_from(event)
                            .map_err(AutoConfigClientError::EventParseError);
                        match msg {
                            Ok(msg) => debug_span!("message").in_scope(|| {
                                let mut changes = { self.as_mut().process_message(msg.clone()) };

                                if !changes.is_empty() {
                                    self.as_mut().changes.append(&mut changes)
                                }
                            }),
                            Err(e) => {
                                error!(error=%e, "failed to parse event");
                                return std::task::Poll::Ready(Some(Err(e)));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        return std::task::Poll::Ready(Some(Err(e.into())));
                    }
                    None => return std::task::Poll::Ready(None),
                },
            };
        }
    }
}
