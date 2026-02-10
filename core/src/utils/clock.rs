use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync + std::fmt::Debug + 'static {
    fn now(&self) -> Instant;
}

#[derive(Debug, Clone, Copy)]
pub struct RealClock;

impl Clock for RealClock {
    #[inline(always)]
    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub fn default_clock() -> Arc<dyn Clock> {
    Arc::new(RealClock)
}

#[derive(Debug)]
pub struct TestClock {
    current: Mutex<Instant>,
}

impl Default for TestClock {
    fn default() -> Self {
        Self::new()
    }
}

impl TestClock {
    pub fn new() -> Self {
        Self {
            current: Mutex::new(Instant::now()),
        }
    }

    pub fn advance(&self, duration: Duration) {
        let mut current = self.current.lock().unwrap();
        *current += duration;
    }
}

impl Clock for TestClock {
    fn now(&self) -> Instant {
        *self.current.lock().unwrap()
    }
}
