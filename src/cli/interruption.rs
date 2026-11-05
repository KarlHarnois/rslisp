use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct Interruption {
    raised: Option<Arc<AtomicBool>>,
}

impl Interruption {
    const HANDLER_LATENCY: Duration = Duration::from_millis(200);
    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    pub fn catch() -> Result<Self, ctrlc::Error> {
        let raised = Arc::new(AtomicBool::new(false));
        let handler_flag = Arc::clone(&raised);
        ctrlc::set_handler(move || handler_flag.store(true, Ordering::SeqCst))?;
        Ok(Self::raised_by(raised))
    }

    fn raised_by(raised: Arc<AtomicBool>) -> Self {
        Self {
            raised: Some(raised),
        }
    }

    pub fn is_raised(&self) -> bool {
        self.raised
            .as_ref()
            .is_some_and(|raised| raised.load(Ordering::SeqCst))
    }

    pub fn caused_a_failure(&self) -> bool {
        self.raised
            .as_ref()
            .is_some_and(Self::raised_within_latency)
    }

    pub fn acknowledge(&self) {
        self.wait_for_the_handler();
        self.clear();
    }

    fn wait_for_the_handler(&self) {
        if let Some(raised) = &self.raised {
            Self::raised_within_latency(raised);
        }
    }

    pub fn clear(&self) {
        if let Some(raised) = &self.raised {
            raised.store(false, Ordering::SeqCst);
        }
    }

    fn raised_within_latency(raised: &Arc<AtomicBool>) -> bool {
        let deadline = Instant::now() + Self::HANDLER_LATENCY;
        while !raised.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Self::POLL_INTERVAL);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct LateHandler {
        flag: Arc<AtomicBool>,
    }

    impl LateHandler {
        const DELAY: Duration = Duration::from_millis(50);

        fn new() -> Self {
            Self {
                flag: Arc::new(AtomicBool::new(false)),
            }
        }

        fn interruption(&self) -> Interruption {
            Interruption::raised_by(Arc::clone(&self.flag))
        }

        fn raise_after_delay(&self) -> thread::JoinHandle<()> {
            let flag = Arc::clone(&self.flag);
            thread::spawn(move || {
                thread::sleep(Self::DELAY);
                flag.store(true, Ordering::SeqCst);
            })
        }
    }

    #[test]
    fn an_acknowledged_interruption_stays_cleared_when_the_handler_runs_late() {
        let handler = LateHandler::new();
        let interruption = handler.interruption();
        let raising = handler.raise_after_delay();
        interruption.acknowledge();
        raising.join().unwrap();
        assert!(!interruption.is_raised());
    }
}
