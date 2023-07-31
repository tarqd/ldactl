use eventsource_client::{ClientBuilder, Error, Event};
#[allow(unused_imports)]
use tracing::{debug, error, info, trace, warn};
mod credential;
mod messages;
mod sse_codec;
