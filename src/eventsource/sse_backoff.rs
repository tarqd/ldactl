use crate::eventsource::retryable::Retryable;
use backoff::backoff::Backoff;
use std::time::Duration;
pub trait WithMinimumBackoff<B>
where
    B: Backoff + Sized,
{
    fn minimum_duration(self, duration: Duration) -> MinimumBackoffDuration<B>;
}

impl<C> WithMinimumBackoff<backoff::exponential::ExponentialBackoff<C>>
    for backoff::exponential::ExponentialBackoff<C>
where
    C: backoff::Clock + Sized,
{
    fn minimum_duration(self, duration: Duration) -> MinimumBackoffDuration<Self> {
        MinimumBackoffDuration::new(self, duration)
    }
}

#[derive(Debug)]
pub struct MinimumBackoffDuration<B: Backoff + Sized> {
    backoff: B,
    minimum_duration: Duration,
}

impl<B> MinimumBackoffDuration<B>
where
    B: Backoff + Sized,
{
    pub fn new(backoff: B, minimum_duration: Duration) -> Self {
        Self {
            backoff,
            minimum_duration,
        }
    }
    pub fn set_minimum_duration(&mut self, minimum_duration: Duration) {
        self.minimum_duration = minimum_duration;
    }
}

impl<B> Backoff for MinimumBackoffDuration<B>
where
    B: Backoff + Sized,
{
    fn next_backoff(&mut self) -> Option<Duration> {
        self.backoff
            .next_backoff()
            .map(|duration| duration.max(self.minimum_duration))
    }

    fn reset(&mut self) {
        self.backoff.reset();
    }
}
