#[allow(unused_imports)]
use tracing::{info, debug, error, warn, trace};
use eventsource_client::{ClientBuilder, Error, Event};
mod credential;
mod messages;



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
       assert!(true)
    }
}
