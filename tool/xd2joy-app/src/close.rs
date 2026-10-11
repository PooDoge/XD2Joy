//! Closing the window while something would be lost: the first close is refused with the reason,
//! a second close soon after goes ahead.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// How long a refused close counts as asked; a second close within it quits.
const AGAIN_WITHIN: Duration = Duration::from_secs(10);

#[derive(Clone, Default)]
pub struct CloseGuard(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    reason: Option<String>,
    asked_at: Option<Instant>,
}

impl CloseGuard {
    /// What closing now would lose; `None` when nothing would. The window sets it on every render.
    pub fn set_reason(&self, reason: Option<String>) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).reason = reason;
    }

    /// `Ok` to close; `Err(reason)` to keep the window open and say why.
    pub fn decide(&self, now: Instant) -> Result<(), String> {
        let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some(reason) = s.reason.clone() else { return Ok(()) };
        if s.asked_at.is_some_and(|t| now.duration_since(t) < AGAIN_WITHIN) {
            tracing::warn!(%reason, "closing anyway");
            return Ok(());
        }
        s.asked_at = Some(now);
        tracing::info!(%reason, "close refused once");
        Err(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_close_soon_after_goes_ahead() {
        let g = CloseGuard::default();
        let t0 = Instant::now();
        assert_eq!(g.decide(t0), Ok(()));
        g.set_reason(Some("updating".into()));
        assert_eq!(g.decide(t0), Err("updating".into()));
        assert_eq!(g.decide(t0 + Duration::from_secs(3)), Ok(()));
        // much later it asks again
        assert!(g.decide(t0 + Duration::from_secs(60)).is_err());
    }
}
