//! Owned by: W7-A
//! RAII terminal and signal cleanup, including unwinding.

use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

struct Restore<F: FnMut()> {
    action: F,
    armed: bool,
}
impl<F: FnMut()> Restore<F> {
    fn restore(&mut self) {
        if self.armed {
            self.armed = false;
            (self.action)();
        }
    }
}
impl<F: FnMut()> Drop for Restore<F> {
    fn drop(&mut self) {
        self.restore();
    }
}
fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(std::io::stdout(), LeaveAlternateScreen, Show);
}
pub(super) struct Guard {
    _restore: Restore<fn()>,
    cancel: Arc<AtomicBool>,
    signals: Vec<signal_hook::SigId>,
}
impl Guard {
    pub(super) fn enter() -> std::io::Result<Self> {
        let mut guard = Self {
            _restore: Restore {
                action: restore,
                armed: true,
            },
            cancel: Arc::new(AtomicBool::new(false)),
            signals: Vec::new(),
        };
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            guard
                .signals
                .push(signal_hook::flag::register(signal, guard.cancel.clone())?);
        }
        enable_raw_mode()?;
        execute!(std::io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(guard)
    }
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self._restore.restore();
        for signal in self.signals.drain(..) {
            signal_hook::low_level::unregister(signal);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restores_once_on_drop_and_explicit_cleanup() {
        let calls = std::cell::Cell::new(0);
        {
            let mut guard = Restore {
                action: || calls.set(calls.get() + 1),
                armed: true,
            };
            guard.restore();
            guard.restore();
        }
        assert_eq!(calls.get(), 1);
    }
    #[test]
    fn restores_on_unwind() {
        let calls = std::cell::Cell::new(0);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = Restore {
                action: || calls.set(calls.get() + 1),
                armed: true,
            };
            panic!("fixture");
        }));
        assert_eq!(calls.get(), 1);
    }
}
