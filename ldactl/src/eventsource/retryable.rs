use super::EventSourceError;
use reqwest::StatusCode;

pub trait Retryable {
    fn is_retryable(&self) -> bool;
}

impl Retryable for EventSourceError {
    fn is_retryable(&self) -> bool {
        match self {
            EventSourceError::RequestCloneError => false,
            EventSourceError::RequestError(e) => e.is_retryable(),
            EventSourceError::MaxRetriesExceeded(..) => false,
            EventSourceError::DecodeError(_) => true,
            EventSourceError::ReadTimeoutElapsed(..) => true,
            // we will treat all i/o errors as retryable here
            EventSourceError::Io(_) => true,
        }
    }
}

impl Retryable for reqwest::Error {
    fn is_retryable(&self) -> bool {
        match self.status() {
            Some(status) => match status {
                _ if status.is_server_error() => true,
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

impl Retryable for std::io::Error {
    fn is_retryable(&self) -> bool {
        match self.kind() {
            std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::AddrInUse
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::UnexpectedEof => true,
            _ => false,
        }
    }
}
