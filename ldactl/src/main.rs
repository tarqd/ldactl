mod credential;
mod messages;

mod autoconfigclient;
mod message_event_source;
use autoconfigclient::ConfigChangeEvent;
use clap::Parser;
use credential::{ClientSideId, ServerSideKey};
use futures::FutureExt;
use futures::{pin_mut, TryStream};
use messages::EnvironmentConfig;
use miette::{miette, Context, Diagnostic, IntoDiagnostic};
use reqwest::ClientBuilder;
use std::collections::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::string::ParseError;
use tempfile::tempfile;
use tokio::sync::oneshot::error::TryRecvError;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tracing::{debug, instrument, trace, Instrument, Span};
use tracing_subscriber::{EnvFilter, FmtSubscriber};
static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

mod eventsource;
use crate::credential::RelayAutoConfigKey;
use crate::credential::{LaunchDarklyCredential, LaunchDarklyCredentialExt};
use crate::eventsource::{EventSource, EventSourceError};
use crate::messages::{Expirable, Expiring};
use std::convert::TryFrom;
use tokio_sse_codec::{Event, Frame, SseDecodeError, SseDecoder};

type ExpirableSDKKey = Expirable<ServerSideKey>;
type ExpiringSDKKey = Expiring<ServerSideKey>;


#[derive(
    clap::ValueEnum, Clone, Default, Debug, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
enum ExecMode {
    ChangeJSON,
    #[default]
    Env,
}

#[derive(Parser, Debug)]
#[command(name = "ldactl")]
#[command(about = "LaunchDarkly Relay AutoConfig CLI", long_about = Some("LaunchDarkly Relay AutoConfig CLI\n\nThis utility is used to fetch and parse the LaunchDarkly Relay AutoConfig stream and write it to a file or execute a command when changes are detected."))]
struct Args {
    #[arg(short = 'k', long, env = "LD_RELAY_AUTO_CONFIG_KEY", value_parser=RelayAutoConfigKey::try_from_str, help = "The LaunchDarkly Relay AutoConfig key to use. See https://launchdarkly.com/docs/sdk/relay-proxy/automatic-configuration")]
    credential: RelayAutoConfigKey,
    #[arg(
        short = 'u',
        long = "stream-uri",
        env = "LD_STREAM_URI",
        default_value = "https://stream.launchdarkly.com/",
        help = "The URI of the LaunchDarkly Relay AutoConfig stream. For Federal: https://stream.launchdarkly.us/ For EU: https://stream.eu.launchdarkly.com"
    )]
    uri: reqwest::Url,
    #[arg(short = 'o', long = "once", default_value = "false", help = "Only run once and exit.")]
    once: bool,
    #[arg(short = 'f', long = "output-file", value_name="OUT_FILE", value_hint=clap::ValueHint::FilePath, env = "LDAC_OUTPUT_FILE", help = "Writes the JSON of all environments to a file. The file will be updated when changes are detected.")]
    output_file: Option<std::path::PathBuf>,
    #[arg(short = 'm', long = "exec-mode", default_value = "env", help = "Mode for the exec command. When change-json, the change event will be written to STDIN. When env, the change event will only be available as environment variables.", env = "LDAC_EXEC_MODE")]
    exec_mode: ExecMode,
    #[arg(short = 'e', long = "exec", help = "Execute a command with the change event. LDAC_* environment variables will be set with values from the change event.", env = "LDAC_EXEC")]
    exec: Option<String>,
    
    #[arg(last = true, help = "Arguments to pass to the exec command.")]
    exec_args: Option<Vec<String>>,
}
#[tokio::main]
async fn main() -> Result<(), miette::Report> {
    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .terminal_links(true)
                .unicode(false)
                .context_lines(3)
                .tab_width(4)
                .build(),
        )
    }))
    .unwrap();
    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    let key = args.credential;
    let client = ClientBuilder::new().build().map_err(|e| miette!(e))?;
    let mut url = args.uri;
    url.path_segments_mut().unwrap().push("relay_auto_config");

    let client = autoconfigclient::AutoConfigClient::new(key);
    pin_mut!(client);

    let (debounce_tx, debounce_rx) = tokio::sync::mpsc::channel(1);
    let (flush_tx, mut flush_rx) = tokio::sync::mpsc::channel(1);
    let file = tokio::spawn(file_write_debouncer(debounce_rx, flush_tx));

    loop {
        tokio::select! {

            _ = flush_rx.recv() => {
                if let Some(path) = args.output_file.as_ref() {
                    write_outfile(path.clone(), client.environments().clone()).await?;
                    debug!(?path, "wrote environments to file");
                }
            }
            result = client.try_next() => {
                if let Some(change) = result? {
                    if args.output_file.is_some() {
                        debounce_tx.send(()).await.into_diagnostic()?;
                    }
                    match change {
                        ConfigChangeEvent::Initialized => {
                            debug!(environment_count=client.environments().len(), "initialized");
                            if args.once {
                                break;
                            }

                        },
                        _ => {
                            if let Some(cmd) = args.exec.as_ref() {
                                let exec_args = args.exec_args.clone().unwrap_or_default();
                                let _ = execute_hook(cmd.clone(), exec_args, change, args.exec_mode.clone()).await;
                            }
                        }
                    }

                }
            }
        }
    }
    Ok(())
}
fn set_event_env_vars(prefix: &str, cmd: &mut std::process::Command, env_config: &EnvironmentConfig) {
    cmd.env(format!("{}_PROJECT_KEY", prefix), env_config.proj_key.as_ref() as &str);
    cmd.env(format!("{}_PROJECT_NAME", prefix), &env_config.proj_name);
    cmd.env(format!("{}_ENV_ID", prefix), env_config.env_id.as_ref() as &str);
    cmd.env(format!("{}_ENV_KEY", prefix), env_config.env_key.as_ref() as &str);
    cmd.env(format!("{}_ENV_NAME", prefix), &env_config.env_name);
    cmd.env(format!("{}_MOBILE_KEY", prefix), env_config.mob_key.as_ref() as &str);
    cmd.env(format!("{}_SDK_KEY", prefix), env_config.sdk_key.current().as_ref() as &str);
    cmd.env(format!("{}_ENV_VERSION", prefix), env_config.version.to_string());
    cmd.env(format!("{}_ENV_DEFAULT_TTL", prefix), env_config.default_ttl.to_string());
    cmd.env(format!("{}_ENV_SECURE_MODE", prefix), env_config.secure_mode.to_string());
}
#[instrument]
fn execute_hook(
    cmd: String,
    args: Vec<String>,
    change_event: ConfigChangeEvent,
    exec_mode: ExecMode,
) -> JoinHandle<Result<(), miette::Report>> {
    // TODO: Use tokio to spawn instead
    // we should also wrap the output in tracing
    let span = Span::current();
    tokio::task::spawn_blocking(move || -> Result<(), miette::Report> {
        let _span = span.enter();
        let mut cmd = std::process::Command::new(cmd);
        cmd.args(args);
        
        // Extract environment-specific data based on the change event type
        match &change_event {
            ConfigChangeEvent::Initialized => {
                cmd.env("LDAC_EVENT_KIND", "initialized");
            }
            ConfigChangeEvent::Insert(env_config) => {
                cmd.env("LDAC_EVENT_KIND", "insert");
                set_event_env_vars("LDAC", &mut cmd, env_config);
            }
            ConfigChangeEvent::Update { current, previous } => {
                cmd.env("LDAC_EVENT_KIND", "update");
                set_event_env_vars("LDAC", &mut cmd, current);
                set_event_env_vars("LDAC_PREV", &mut cmd, previous);
            }
            ConfigChangeEvent::Delete(env_config) => {
                cmd.env("LDAC_EVENT_KIND", "delete");
                set_event_env_vars("LDAC_", &mut cmd, env_config);
            }
        }
        
        // Only pipe stdin if we're in ChangeJSON mode
        if matches!(exec_mode, ExecMode::ChangeJSON) {
            cmd.stdin(std::process::Stdio::piped());
        }
        
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        debug!("executing hook command");
        let mut child = cmd.spawn().into_diagnostic()?;
        
        // Only write to stdin if we're in ChangeJSON mode
        if matches!(exec_mode, ExecMode::ChangeJSON) {
            if let Some(stdin) = child.stdin.as_mut() {
                let mut writer = BufWriter::new(stdin);
                serde_json::to_writer(&mut writer, &change_event).into_diagnostic()?;
                writer.flush().into_diagnostic()?;
            }
        }
        
        child
            .wait()
            .into_diagnostic()
            .context("hook command failed")?;
        Ok(())
    })
}

#[instrument(target="file_output", skip(environments), fields(environment_count = environments.len()))]
async fn write_outfile(
    path: PathBuf,
    environments: HashMap<ClientSideId, EnvironmentConfig>,
) -> Result<(), miette::Report> {
    let mut tmp = tempfile::NamedTempFile::new().map_err(|e| miette!(e))?;
    let writer = BufWriter::new(tmp.as_file_mut());
    serde_json::to_writer_pretty(writer, &environments).map_err(|e| miette!(e))?;
    tmp.flush().map_err(|e| miette!(e))?;

    std::fs::rename(tmp.path(), path).map_err(|e| miette!(e))?;
    Ok(())
}
#[instrument(target = "file_output", skip(rx, tx))]
async fn file_write_debouncer(
    mut rx: tokio::sync::mpsc::Receiver<()>,
    tx: tokio::sync::mpsc::Sender<()>,
) {
    let duration = std::time::Duration::from_millis(500);
    let mut needs_flush = false;
    loop {
        match tokio::time::timeout(duration, rx.recv()).await {
            Ok(Some(_)) => {
                if !needs_flush {
                    trace!("file output flush scheduled");
                    needs_flush = true;
                }
            }
            Ok(None) => {
                if needs_flush {
                    trace!("file output flush requested");
                    tx.send(()).await.unwrap();
                    needs_flush = false;
                }
            }
            Err(_) => {
                if needs_flush {
                    trace!("file output flush requested");
                    tx.send(()).await.unwrap();
                    needs_flush = false;
                }
            }
        }
    }
}
