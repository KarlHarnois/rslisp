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
        Ok(Self {
            raised: Some(raised),
        })
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
