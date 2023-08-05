use backoff::backoff::Backoff;
use std::{ops::DerefMut, time::Duration};
pub trait WithMinimumBackoff<B>
where
    B: std::ops::Deref<Target = dyn Backoff> + Sized,
{
    fn with_minimum_duration(self, duration: Duration) -> MinimumBackoffDuration<B>;
}

impl<B> WithMinimumBackoff<B> for B
where
    B: std::ops::Deref<Target = dyn Backoff> + Sized,
{
    fn with_minimum_duration(self, duration: Duration) -> MinimumBackoffDuration<Self> {
        MinimumBackoffDuration::new(self, duration)
    }
}

#[derive(Debug)]
pub struct MinimumBackoffDuration<B>
where
    B: std::ops::Deref<Target = dyn Backoff> + Sized,
{
    backoff: B,
    minimum_duration: Duration,
}

impl<B> MinimumBackoffDuration<B>
where
    B: std::ops::Deref<Target = dyn Backoff>,
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
    B: std::ops::DerefMut<Target = dyn Backoff> + Sized,
{
    fn next_backoff(&mut self) -> Option<Duration> {
        self.backoff
            .deref_mut()
            .next_backoff()
            .map(|duration| duration.max(self.minimum_duration))
    }

    fn reset(&mut self) {
        self.backoff.deref_mut().reset();
    }
}
