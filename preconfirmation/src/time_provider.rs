use std::time::{SystemTime, UNIX_EPOCH};

#[cfg_attr(test, mockall::automock)]
pub trait ITimeProvider {
    fn now(&self) -> SystemTime;
    fn timestamp_in_s(&self) -> u64;
    fn timestamp_in_ms(&self) -> u64;
}

pub struct SystemTimeProvider;

impl Default for SystemTimeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemTimeProvider {
    pub fn new() -> Self {
        Self {}
    }
}

impl ITimeProvider for SystemTimeProvider {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn timestamp_in_s(&self) -> u64 {
        self.now().duration_since(UNIX_EPOCH).unwrap().as_secs()
    }

    fn timestamp_in_ms(&self) -> u64 {
        self.now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .try_into()
            .expect("Timestamp in ms exceeds u64")
    }
}
