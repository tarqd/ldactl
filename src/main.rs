mod credential;
mod messages;

use futures::{ready, stream::FusedStream, Stream, StreamExt, TryStreamExt};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use messages::Message;
use miette::{miette, Error, Result};

use eventsource_client::Error as EventSourceError;
use pin_project::pin_project;
use pretty_env_logger;

#[tokio::main]
async fn main() {}
