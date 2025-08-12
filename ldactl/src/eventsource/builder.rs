use std::{convert::Infallible, fmt};

use backoff::backoff::Backoff;
use eventsource_client::ClientBuilder;
use reqwest::{
    header::{self, HeaderName, HeaderValue, InvalidHeaderName, InvalidHeaderValue},
    ClientBuilder as ReqwestClientBuilder, Url,
};
use thiserror::Error;
use tracing::{debug_span, Span};

use super::{sse_backoff::WithMinimumBackoff, EventSource};
mod http {
    pub use reqwest::header;
    pub use reqwest::Error;
}

#[derive(Debug, Error)]
pub enum EventSourceBuilderError {
    #[error("failed to build request")]
    Request(#[from] http::Error),
    #[error("invalid header value")]
    InvalidHeaderValue(#[from] InvalidHeaderValue),
    #[error("invalid header name")]
    InvalidHeaderName(#[from] InvalidHeaderName),
    #[error("error while building headers: {0}")]
    Other(#[source] Box<dyn std::error::Error>),
}
impl From<Infallible> for EventSourceBuilderError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

pub struct EventSourceBuilder {
    read_timeout_duration: std::time::Duration,
    backoff: Option<Box<dyn backoff::backoff::Backoff>>,
    client_builder: ReqwestClientBuilder,
    request: Result<reqwest::Request, EventSourceBuilderError>,
    last_event_id: Option<String>,
    error: Option<EventSourceBuilderError>,
    redirect_policy: reqwest::redirect::Policy,
}

impl EventSourceBuilder {
    pub fn from_request(request: reqwest::Request) -> Self {
        let mut request = request;
        request
            .headers_mut()
            .insert("accept", "text/event-stream".parse().unwrap());
        request
            .headers_mut()
            .insert("cache-control", "no-cache".parse().unwrap());
        Self {
            read_timeout_duration: std::time::Duration::from_secs(5 * 60),
            backoff: None,
            client_builder: ReqwestClientBuilder::new(),
            request: Ok(request),
            last_event_id: None,
            error: None,
            redirect_policy: reqwest::redirect::Policy::default(),
        }
    }
    pub fn new(url: Url) -> Self {
        Self::from_request(reqwest::Request::new(reqwest::Method::GET, url))
    }
    pub fn get(url: Url) -> Self {
        Self::from_request(reqwest::Request::new(reqwest::Method::GET, url))
    }
    pub fn post(url: Url) -> Self {
        Self::from_request(reqwest::Request::new(reqwest::Method::POST, url))
    }
    pub fn report(url: Url) -> Self {
        Self::from_request(reqwest::Request::new(
            reqwest::Method::from_bytes(b"REPORT").unwrap(),
            url,
        ))
    }
    pub fn put(url: Url) -> Self {
        Self::from_request(reqwest::Request::new(reqwest::Method::PUT, url))
    }
    pub fn patch(url: Url) -> Self {
        Self::from_request(reqwest::Request::new(reqwest::Method::PATCH, url))
    }

    pub fn with_client_builder(mut self, client_builder: ReqwestClientBuilder) -> Self {
        self.client_builder = client_builder;
        self
    }
    pub fn read_timeout(mut self, read_timeout: std::time::Duration) -> Self {
        self.read_timeout_duration = read_timeout;
        self
    }
    pub fn with_backoff_strategy<T>(mut self, backoff_strategy: T) -> Self
    where
        T: Backoff + Sized + 'static,
    {
        self.backoff = Some(Box::new(backoff_strategy));
        self
    }
    pub fn with_expontential_backoff(
        mut self,
        initial_delay: std::time::Duration,
        max_delay: std::time::Duration,
        max_elapsed_time: std::time::Duration,
    ) -> Self {
        self.with_backoff_strategy(
            backoff::ExponentialBackoffBuilder::new()
                .with_initial_interval(initial_delay)
                .with_max_interval(max_delay)
                .with_max_elapsed_time(Some(max_elapsed_time))
                .build(),
        )
    }
    pub fn last_event(mut self, last_event_id: Option<String>) -> Self {
        self.last_event_id = last_event_id;
        self
    }
    // copied from reqwest::RequestBuilder
    // mit license

    /// Add a `Header` to this Request.
    pub fn header<K, V>(self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<EventSourceBuilderError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<EventSourceBuilderError>,
    {
        self.header_sensitive(key, value, false)
    }
    /// Add a `Header` to this Request with ability to define if header_value is sensitive.
    fn header_sensitive<K, V>(mut self, key: K, value: V, sensitive: bool) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<EventSourceBuilderError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<EventSourceBuilderError>,
    {
        let mut error = None;
        if let Ok(ref mut req) = self.request {
            match <HeaderName as TryFrom<K>>::try_from(key) {
                Ok(key) => match <HeaderValue as TryFrom<V>>::try_from(value) {
                    Ok(mut value) => {
                        // We want to potentially make an unsensitive header
                        // to be sensitive, not the reverse. So, don't turn off
                        // a previously sensitive header.
                        if sensitive {
                            value.set_sensitive(true);
                        }
                        req.headers_mut().append(key, value);
                    }
                    Err(e) => error = Some(e.into()),
                },
                Err(e) => error = Some(e.into()),
            };
        }
        if let Some(err) = error {
            self.request = Err(err);
        }
        self
    }
    /// Add a set of Headers to the existing ones on this Request.
    ///
    /// The headers will be merged in to any already set.
    pub fn headers(mut self, headers: reqwest::header::HeaderMap) -> Self {
        if let Ok(ref mut req) = self.request {
            util::replace_headers(req.headers_mut(), headers)
        }
        self
    }

    /// Enable HTTP bearer authentication.
    /// Sets `Authorization: Bearer <token>`.
    pub fn bearer_auth<T>(self, token: T) -> Self
    where
        T: fmt::Display,
    {
        let header_value = format!("Bearer {}", token);
        self.header_sensitive(header::AUTHORIZATION, header_value, true)
    }
    /// Enable HTTP bearer authentication.
    /// Sets the `Authorization: <credentials>`.
    pub fn authorization<V>(self, credential: V) -> Self
    where
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<EventSourceBuilderError>,
    {
        self.header_sensitive(header::AUTHORIZATION, credential, true)
    }
    /// Set the request body.
    pub fn body<T: Into<reqwest::Body> + Clone>(mut self, body: T) -> Self {
        if let Ok(ref mut req) = self.request {
            *req.body_mut() = Some(body.into());
        }
        self
    }
    /// Enables a request timeout.
    ///
    /// The timeout is applied from when the request starts connecting until the
    /// response body has finished. It affects only this request and overrides
    /// the timeout configured using `ClientBuilder::timeout()`.
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        if let Ok(ref mut req) = self.request {
            *req.timeout_mut() = Some(timeout);
        }
        self
    }
    pub fn redirect(mut self, policy: reqwest::redirect::Policy) -> Self {
        self.redirect_policy = policy;
        self
    }
    pub fn build(self) -> Result<super::EventSource, EventSourceBuilderError> {
        let req = self.request?;

        let url = std::sync::Arc::new(std::sync::Mutex::new(Some(req.url().clone())));
        let redirect_policy = {
            let url = url.clone();
            let inner_redirect_policy = self.redirect_policy;
            reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.status() == reqwest::StatusCode::MOVED_PERMANENTLY {
                    let mut url = url.lock().unwrap();
                    *url = Some(attempt.url().clone());
                }
                inner_redirect_policy.redirect(attempt)
            })
        };
        let client = self.client_builder.redirect(redirect_policy).build()?;
        let backoff = self
            .backoff
            .unwrap_or(Box::new(backoff::ExponentialBackoff::default()));
        let last_event_id = self.last_event_id;
        let request_builder = reqwest::RequestBuilder::from_parts(client, req);

        Ok(EventSource {
            request_builder,
            backoff: backoff.with_minimum_duration(std::time::Duration::ZERO),
            last_event_id,
            retry_url: url,
            state: super::state_util::EventSourceState::Initial,
            read_timeout: self.read_timeout_duration,
            retry_attempts: 0,
            is_retrying: false,
        })
    }
}

mod util {
    use reqwest::header::{Entry, HeaderMap, OccupiedEntry};

    use super::*;
    pub(crate) fn replace_headers(dst: &mut HeaderMap, src: HeaderMap) {
        // IntoIter of HeaderMap yields (Option<HeaderName>, HeaderValue).
        // The first time a name is yielded, it will be Some(name), and if
        // there are more values with the same name, the next yield will be
        // None.

        let mut prev_entry: Option<OccupiedEntry<_>> = None;
        for (key, value) in src {
            match key {
                Some(key) => match dst.entry(key) {
                    Entry::Occupied(mut e) => {
                        e.insert(value);
                        prev_entry = Some(e);
                    }
                    Entry::Vacant(e) => {
                        let e = e.insert_entry(value);
                        prev_entry = Some(e);
                    }
                },
                None => match prev_entry {
                    Some(ref mut entry) => {
                        entry.append(value);
                    }
                    None => unreachable!("HeaderMap::into_iter yielded None first"),
                },
            }
        }
    }
}
