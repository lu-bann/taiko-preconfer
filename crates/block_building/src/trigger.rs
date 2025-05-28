#[cfg(test)]
use mockall::automock;
use std::time::Duration;
use tokio::time::sleep;

#[cfg_attr(test, automock)]
pub trait Triggerable {
    fn trigger<E: 'static>(&self) -> impl Future<Output = Result<(), E>>;
}

#[cfg_attr(test, automock)]
pub trait Trigger {
    fn run<E: 'static, T: Triggerable + 'static>(
        &self,
        f: T,
    ) -> impl Future<Output = Result<(), E>>;
}

pub struct PeriodicTrigger {
    period_duration: Duration,
    work_duration: Duration,
}

impl PeriodicTrigger {
    pub fn new(period_duration: Duration, work_duration: Duration) -> Self {
        Self {
            period_duration,
            work_duration,
        }
    }
}

impl Trigger for PeriodicTrigger {
    async fn run<E: 'static, T: Triggerable + 'static>(&self, t: T) -> Result<(), E> {
        loop {
            t.trigger().await?;
            sleep(self.period_duration - self.work_duration).await;
        }
    }
}
