//! Priority-aware job gate: the hard concurrency budget for `rustc` processes.
//!
//! On the reference machine (16 Go RAM, ~2 Go free) spawning several compilers
//! measures paging, not work. The gate makes the budget explicit and testable:
//! at most `max` analyses run at once, and interactive requests are admitted
//! before background ones when the gate is saturated.

use crate::config::Priority;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

struct Waiter {
    seq: u64,
    priority: Priority,
}

#[derive(Default)]
struct GateState {
    active: usize,
    peak: usize,
    waiters: Vec<Waiter>,
}

/// Bounded, priority-aware admission gate.
pub struct JobGate {
    max: usize,
    state: Mutex<GateState>,
    available: Condvar,
    next_seq: AtomicU64,
}

impl JobGate {
    pub fn new(max: usize) -> Self {
        Self {
            max: max.max(1),
            state: Mutex::new(GateState::default()),
            available: Condvar::new(),
            next_seq: AtomicU64::new(0),
        }
    }

    pub fn max(&self) -> usize {
        self.max
    }

    /// Blocks until a slot is available. Interactive waiters are admitted first;
    /// ties are broken by arrival order (FIFO), so background work cannot starve.
    pub fn acquire(&self, priority: Priority) -> GateGuard<'_> {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.waiters.push(Waiter { seq, priority });
        loop {
            let best = state
                .waiters
                .iter()
                .max_by(|a, b| {
                    a.priority
                        .cmp(&b.priority)
                        .then_with(|| b.seq.cmp(&a.seq))
                })
                .map(|waiter| waiter.seq);
            if state.active < self.max && best == Some(seq) {
                state.waiters.retain(|waiter| waiter.seq != seq);
                state.active += 1;
                state.peak = state.peak.max(state.active);
                return GateGuard { gate: self };
            }
            state = match self.available.wait(state) {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }

    pub fn active(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.active)
            .unwrap_or(usize::MAX)
    }

    pub fn peak(&self) -> usize {
        self.state.lock().map(|state| state.peak).unwrap_or(0)
    }
}

/// Releases its slot on drop, including on panic paths.
pub struct GateGuard<'a> {
    gate: &'a JobGate,
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        let mut state = match self.gate.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.active = state.active.saturating_sub(1);
        self.gate.available.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn gate_never_exceeds_its_budget() {
        let gate = Arc::new(JobGate::new(1));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let gate = Arc::clone(&gate);
            let peak = Arc::clone(&peak);
            handles.push(std::thread::spawn(move || {
                let guard = gate.acquire(Priority::Normal);
                let now = gate.active();
                peak.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                drop(guard);
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(gate.peak(), 1);
    }

    #[test]
    fn interactive_is_admitted_before_background() {
        let gate = Arc::new(JobGate::new(1));
        // Hold the only slot.
        let held = gate.acquire(Priority::Normal);
        let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));

        // Background waiter arrives *before* the interactive one.
        let background_gate = Arc::clone(&gate);
        let background_order = Arc::clone(&order);
        let background = std::thread::spawn(move || {
            let guard = background_gate.acquire(Priority::Background);
            background_order.lock().unwrap().push("background");
            drop(guard);
        });
        std::thread::sleep(Duration::from_millis(20));
        let interactive_gate = Arc::clone(&gate);
        let interactive_order = Arc::clone(&order);
        let interactive = std::thread::spawn(move || {
            let guard = interactive_gate.acquire(Priority::Interactive);
            interactive_order.lock().unwrap().push("interactive");
            drop(guard);
        });
        std::thread::sleep(Duration::from_millis(20));
        drop(held);
        background.join().unwrap();
        interactive.join().unwrap();
        assert_eq!(
            order.lock().unwrap().as_slice(),
            ["interactive", "background"],
            "interactive request must be admitted first"
        );
    }
}
