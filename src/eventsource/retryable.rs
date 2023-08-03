use reqwest::StatusCode;

use crate::eventsource::sse_codec::SSEDecodeError;

pub trait Retryable {
    fn is_retryable(&self) -> bool;
}

impl Retryable for SSEDecodeError {
    fn is_retryable(&self) -> bool {
        true
    }
}

impl Retryable for reqwest::Error {
    fn is_retryable(&self) -> bool {
        match self.status() {
            Some(status) => match status {
                _ if !status.is_client_error() => true,
                StatusCode::BAD_REQUEST
                | StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS => true,
                _ => false,
            },
            None => {
                if self.is_connect()
                    || self.is_timeout()
                    || self.is_decode()
                    || (!self.is_request() && self.is_body())
                {
                    true
                } else {
                    false
                }
            }
        }
    }
}
