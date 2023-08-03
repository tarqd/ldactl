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
use crate::eventsource::sse_codec::{Event, Item, SSECodec, SSEDecodeError};
use crate::eventsource::{EventSource, EventSourceError};
use crate::messages::{Expirable, Expiring};
use std::convert::TryFrom;

type ExpirableSDKKey = Expirable<ServerSideKey>;
type ExpiringSDKKey = Expiring<ServerSideKey>;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short = 'k', long, env = "LD_RELAY_AUTO_CONFIG_KEY", value_parser=RelayAutoConfigKey::try_from_str)]
    credential: RelayAutoConfigKey,
    #[arg(
        short = 'u',
        long = "stream-uri",
        env = "LD_STREAM_URI",
        default_value = "https://stream.launchdarkly.com/"
    )]
    uri: reqwest::Url,
    #[arg(short = 'o', long = "once", default_value = "false")]
    once: bool,
    #[arg(short = 'f', long = "output-file", value_name="OUT_FILE", value_hint=clap::ValueHint::FilePath, env = "LD_AUTO_CONFIG_OUTPUT_FILE")]
    output_file: Option<std::path::PathBuf>,

    #[arg(short = 'e', long = "exec")]
    exec: Option<String>,
    #[arg(last = true)]
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

    let request = client.get(url).header("user-agent", APP_USER_AGENT);
    let client = autoconfigclient::AutoConfigClient::with_request_builder(key, request);
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
                                let args = args.exec_args.clone().unwrap_or_default();
                                execute_hook(cmd.clone(), args, change).await;
                            }
                        }
                    }

                }
            }
        }
    }
    Ok(())
}

#[instrument]
async fn execute_hook(
    cmd: String,
    args: Vec<String>,
    change_event: ConfigChangeEvent,
) -> JoinHandle<Result<(), miette::Report>> {
    // TODO: Use tokio to spawn instead
    // we should also wrap the output in tracing
    let span = Span::current();
    tokio::task::spawn_blocking(move || -> Result<(), miette::Report> {
        let _span = span.enter();
        let mut cmd = std::process::Command::new(cmd);
        cmd.args(args);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        debug!("executing hook command");
        let mut child = cmd.spawn().into_diagnostic()?;
        {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| miette!("failed to write to hook command stdin"))?;
            let mut writer = BufWriter::new(stdin);
            serde_json::to_writer(&mut writer, &change_event).into_diagnostic()?;
            writer.flush().into_diagnostic()?;
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
