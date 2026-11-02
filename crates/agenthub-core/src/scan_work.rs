//! Cancellation and memory accounting for read-only cleanup previews.
use anyhow::{ensure, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub(crate) const SCAN_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
#[derive(Clone)]
pub(crate) struct ScanWork {
    cancelled: Arc<AtomicBool>,
    touched: Arc<Mutex<Instant>>,
    expiring: bool,
}

impl Default for ScanWork {
    fn default() -> Self {
        Self::new(false)
    }
}

impl ScanWork {
    pub(crate) fn new(expiring: bool) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            touched: Arc::new(Mutex::new(Instant::now())),
            expiring,
        }
    }

    pub(crate) fn check(&self) -> Result<()> {
        ensure!(!self.cancelled.load(Ordering::Acquire), "扫描已停止");
        ensure!(!self.expired(), "扫描已过期，请重新扫描");
        Ok(())
    }

    pub(crate) fn touch(&self) {
        if let Ok(mut touched) = self.touched.lock() {
            *touched = Instant::now();
        }
    }

    pub(crate) fn expired(&self) -> bool {
        self.expiring
            && self
                .touched
                .lock()
                .map_or(true, |time| time.elapsed() >= SCAN_IDLE_TIMEOUT)
    }

    pub(crate) fn idle_remaining(&self) -> Duration {
        self.touched.lock().map_or(Duration::ZERO, |time| {
            SCAN_IDLE_TIMEOUT.saturating_sub(time.elapsed())
        })
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactivity_expires_and_polling_refreshes_the_deadline() {
        let work = ScanWork::new(true);
        *work.touched.lock().unwrap() = Instant::now() - SCAN_IDLE_TIMEOUT;
        assert!(work.expired());
        assert_eq!(work.idle_remaining(), Duration::ZERO);
        assert!(work.check().is_err());
        work.touch();
        assert!(!work.expired());
        assert!(work.idle_remaining() > SCAN_IDLE_TIMEOUT - Duration::from_secs(1));
        work.cancel();
        work.touch();
        assert!(work.check().is_err());
    }
}
