mod builder;
mod errorext;
mod eventsource;
mod retryable;
mod sse_backoff;
mod state_util;

pub use builder::{EventSourceBuilder, EventSourceBuilderError};
pub use eventsource::{EventSource, EventSourceError};
pub type Result<T> = std::result::Result<T, EventSourceError>;

mod backoff {
    pub use backoff::backoff::Backoff;
    pub use backoff::ExponentialBackoff;
    pub use backoff::ExponentialBackoffBuilder;
}

// re-exports from reqwest
pub mod http {
    pub mod header {
        pub use reqwest::header::{
            HeaderMap, HeaderName, HeaderValue, InvalidHeaderName, InvalidHeaderValue,
        };
    }
    mod redirect {
        pub use reqwest::redirect::{Attempt, Policy};
    }
    pub use reqwest::{Body, Client, ClientBuilder, IntoUrl, Request, RequestBuilder, Url};
    pub use reqwest::{Method, StatusCode, Version};
}
