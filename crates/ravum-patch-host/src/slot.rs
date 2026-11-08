//! Lock-free publication slot (M3), hardened for the audio thread (MVP-0b).
//!
//! The audio path must not take a mutex, not allocate, **not free**, and not wait.
//! This slot gives a reader one atomic increment and no lock, while a writer replaces
//! the pointer.
//!
//! # Two reader APIs, one of which destroys
//!
//! * [`ArcSlot::load_owning`] returns an owning `Arc<T>`. It is fine on any control thread,
//!   but **forbidden on an audio callback**: releasing that `Arc` may be the last
//!   reference, so the payload's `Drop` — including a deferred-library release — would
//!   run on the callback. `owning_load_can_drop_on_the_reader_thread` demonstrates the
//!   hazard on purpose so the rule is not a comment nobody tests.
//! * [`ArcSlot::enter`] returns a [`ReadGuard`]: a non-owning borrow that costs exactly
//!   two atomic operations and *cannot* drop the payload, because it never touches the
//!   `Arc` strong count. This is the only reader API an audio callback may use.
//!
//! # Why the `readers` counter is enough
//!
//! The only hazard window is between "a reader observed the old pointer" and "the
//! reader has announced itself". The reader announces itself **before** reading the
//! pointer, and the writer, after swapping, waits for that announcement window to
//! drain before releasing the reference it just removed. Announcement is also what
//! keeps an in-flight call alive: the writer's `store` returns the retired `Arc` only
//! after `readers == 0`, so the writer frees the retired payload, never a reader.
//!
//! A writer therefore waits only for readers to leave, which is bounded by the caller's
//! block length. It never runs on the audio thread: `store` is a control-thread
//! operation.
//!
//! # Memory ordering
//!
//! Everything is `SeqCst`. `Acquire`/`Release` would be sufficient on the reader and
//! `AcqRel` on the writer, but sequential consistency costs a few nanoseconds per call
//! — negligible against the measured 1–2 µs block — and makes the "readers == 0 after
//! the swap" reasoning reviewable without a model-checker. This is an explicit
//! trade-off, not an oversight.

use std::ops::Deref;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

pub struct ArcSlot<T> {
    ptr: AtomicPtr<T>,
    readers: AtomicUsize,
}

/// A non-owning read guard: two atomics, no allocation, no deallocation, no `Arc`
/// reference-count change.
///
/// The guard is deliberately `!Send` (it holds a `*const T`), so it cannot be moved to
/// another thread; it must be released on the thread that took it.
pub struct ReadGuard<'a, T> {
    slot: &'a ArcSlot<T>,
    ptr: *const T,
}

impl<'a, T> ReadGuard<'a, T> {
    /// The raw pointer is only exposed for FFI-free inspection; normal reads go
    /// through `Deref`.
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: while the guard is alive, `readers` is non-zero, and a writer only
        // releases the previous `Arc` after observing `readers == 0`. The slot itself
        // cannot be dropped while it is borrowed. So the payload is alive.
        unsafe { &*self.ptr }
    }
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        // SAFETY: this guard performed exactly one `fetch_add` in `enter`.
        self.slot.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

impl<T> ArcSlot<T> {
    pub fn new(value: Arc<T>) -> Self {
        Self {
            ptr: AtomicPtr::new(Arc::into_raw(value) as *mut T),
            readers: AtomicUsize::new(0),
        }
    }

    /// Owning load: **not real-time safe** (the returned `Arc` can be the last
    /// reference). Use [`ArcSlot::enter`] on an audio callback.
    pub fn load_owning(&self) -> Arc<T> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        let raw = self.ptr.load(Ordering::SeqCst);
        // SAFETY: the slot permanently owns one strong reference, and a writer only
        // releases the previous value after observing `readers == 0`, i.e. after
        // every reader that could have observed `raw` has incremented the count.
        unsafe {
            Arc::increment_strong_count(raw);
            let arc = Arc::from_raw(raw);
            self.readers.fetch_sub(1, Ordering::SeqCst);
            arc
        }
    }

    /// Real-time reader entry point: one atomic increment, one pointer load, no
    /// allocation, no free, no strong-count change, no blocking.
    ///
    /// The returned guard borrows the payload; the writer cannot free it before the
    /// guard is dropped.
    pub fn enter(&self) -> ReadGuard<'_, T> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        let ptr = self.ptr.load(Ordering::SeqCst);
        ReadGuard { slot: self, ptr }
    }

    /// Publish `value` and return the previous one (still owned, caller drops it).
    ///
    /// **Control-thread only.** Blocks until every in-flight reader has left, so the
    /// retired payload is never freed under a reader and never freed by a reader.
    ///
    /// Writers must be serialized by the caller: two concurrent writers would not
    /// corrupt memory, but would make the "current" version ambiguous.
    pub fn store(&self, value: Arc<T>) -> Arc<T> {
        let raw = Arc::into_raw(value) as *mut T;
        let old = self.ptr.swap(raw, Ordering::SeqCst);
        while self.readers.load(Ordering::SeqCst) != 0 {
            std::hint::spin_loop();
        }
        // SAFETY: the slot owned exactly one strong reference to `old`.
        unsafe { Arc::from_raw(old) }
    }
}

impl<T> Drop for ArcSlot<T> {
    fn drop(&mut self) {
        // SAFETY: the slot owns one strong reference and is being destroyed. `&mut self`
        // proves no `ReadGuard` (which borrows the slot) is alive.
        unsafe {
            drop(Arc::from_raw(self.ptr.load(Ordering::Relaxed)));
        }
    }
}

// SAFETY: the slot only hands out `Arc<T>`; `T: Send + Sync` is the same bound `Arc`
// itself requires for sharing across threads.
unsafe impl<T: Send + Sync> Send for ArcSlot<T> {}
unsafe impl<T: Send + Sync> Sync for ArcSlot<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::sync::Mutex;
    use std::thread::{self, ThreadId};
    use std::time::{Duration, Instant};

    struct Tracked {
        id: u64,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Records which thread performed the drop: the whole point of the hardening.
    struct ThreadTracked {
        drops: Arc<Mutex<Vec<ThreadId>>>,
    }

    impl Drop for ThreadTracked {
        fn drop(&mut self) {
            self.drops.lock().expect("drops lock").push(thread::current().id());
        }
    }

    #[test]
    fn readers_never_observe_a_freed_value_and_nothing_leaks() {
        let drops = Arc::new(AtomicUsize::new(0));
        let slot = Arc::new(ArcSlot::new(Arc::new(Tracked {
            id: 0,
            drops: Arc::clone(&drops),
        })));

        let mut readers = Vec::new();
        for _ in 0..4 {
            let slot = Arc::clone(&slot);
            readers.push(thread::spawn(move || {
                let mut last = 0u64;
                for _ in 0..5_000 {
                    let value = slot.load_owning();
                    // The writer publishes increasing ids, so observations are monotonic.
                    assert!(value.id >= last, "observed {} after {}", value.id, last);
                    last = value.id;
                    std::hint::black_box(value.id);
                }
            }));
        }

        for id in 1..=200u64 {
            let old = slot.store(Arc::new(Tracked {
                id,
                drops: Arc::clone(&drops),
            }));
            drop(old);
        }
        for reader in readers {
            reader.join().expect("reader");
        }

        // 200 values retired by the writer, all freed once no reader held them.
        assert_eq!(drops.load(Ordering::SeqCst), 200);
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 201, "the last value must be freed");
    }

    #[test]
    fn a_held_arc_outlives_any_number_of_swaps() {
        let drops = Arc::new(AtomicUsize::new(0));
        let slot = ArcSlot::new(Arc::new(Tracked {
            id: 0,
            drops: Arc::clone(&drops),
        }));
        let held = slot.load_owning();
        for id in 1..=10u64 {
            drop(slot.store(Arc::new(Tracked {
                id,
                drops: Arc::clone(&drops),
            })));
            assert_eq!(held.id, 0);
        }
        assert_eq!(drops.load(Ordering::SeqCst), 9, "held value is still alive");
        drop(held);
        assert_eq!(drops.load(Ordering::SeqCst), 10);
    }

    /// This test is a demonstration of the risk, not a desirable behaviour: the
    /// **owning** load can be the last reference, so its payload is dropped on the
    /// reader thread. That is precisely why `BlockHost::process_rt` uses `enter`.
    #[test]
    fn owning_load_can_drop_on_the_reader_thread() {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let slot = Arc::new(ArcSlot::new(Arc::new(ThreadTracked {
            drops: Arc::clone(&drops),
        })));

        let (reader, loaded, release) = {
            let slot = Arc::clone(&slot);
            let (loaded_tx, loaded_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let handle = thread::spawn(move || {
                // Exactly what a naive audio path does: load, use, release.
                let value = slot.load_owning();
                // Deterministic interleaving: the reader owns the *old* value and waits
                // until the writer has released its own reference, so the reader's drop
                // really is the last one.
                loaded_tx.send(()).expect("loaded");
                release_rx.recv().expect("release");
                std::hint::black_box(&*value);
                drop(value);
            });
            (handle, loaded_rx, release_tx)
        };
        loaded.recv().expect("reader loaded the old value");
        // Retire and release the slot's reference from the control thread. The reader
        // is now the last holder of the retired value.
        let retired = slot.store(Arc::new(ThreadTracked {
            drops: Arc::clone(&drops),
        }));
        drop(retired);
        release.send(()).expect("release the reader");
        reader.join().expect("reader");

        let observed = drops.lock().expect("drops lock").clone();
        assert_eq!(observed.len(), 1, "one retired value is dropped");
        assert_ne!(
            observed[0],
            thread::current().id(),
            "the owning load dropped the retired value on the reader thread: this is \
             the hazard the guard API removes"
        );
    }

    #[test]
    fn a_guard_never_performs_the_last_drop_and_the_writer_waits_for_it() {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let slot = Arc::new(ArcSlot::new(Arc::new(ThreadTracked {
            drops: Arc::clone(&drops),
        })));

        let entered = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let reader = {
            let slot = Arc::clone(&slot);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            thread::spawn(move || {
                let guard = slot.enter();
                entered.store(true, Ordering::SeqCst);
                while !release.load(Ordering::SeqCst) {
                    thread::yield_now();
                }
                std::hint::black_box(&*guard);
                drop(guard);
            })
        };

        while !entered.load(Ordering::SeqCst) {
            thread::yield_now();
        }
        // A writer would spin until the guard leaves; ask a helper to release it after
        // 60 ms so the wait is observable without deadlocking the writer.
        let helper = {
            let release = Arc::clone(&release);
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(60));
                release.store(true, Ordering::SeqCst);
            })
        };

        let started = Instant::now();
        let retired = slot.store(Arc::new(ThreadTracked {
            drops: Arc::clone(&drops),
        }));
        let waited = started.elapsed();
        drop(retired);
        helper.join().expect("helper");
        reader.join().expect("reader");

        assert!(
            waited >= Duration::from_millis(30),
            "the writer must wait for the reader's guard, waited only {waited:?}"
        );
        let observed = drops.lock().expect("drops lock").clone();
        assert_eq!(observed.len(), 1, "one retired value is dropped");
        assert_eq!(
            observed[0],
            thread::current().id(),
            "the retired payload must be freed by the writer, never by the reader"
        );
    }

    #[test]
    fn the_last_leaving_guard_decides_when_the_writer_may_free() {
        let drops = Arc::new(Mutex::new(Vec::new()));
        let slot = Arc::new(ArcSlot::new(Arc::new(ThreadTracked {
            drops: Arc::clone(&drops),
        })));
        let held = slot.enter();

        // A second reader leaves before the writer can free anything.
        {
            let slot_ref = Arc::clone(&slot);
            let _second = slot_ref.enter();
        }

        let started = Instant::now();
        let (store, writer_id) = {
            let slot = Arc::clone(&slot);
            let drops = Arc::clone(&drops);
            let (tx, rx) = std::sync::mpsc::channel();
            let handle = thread::spawn(move || {
                tx.send(thread::current().id()).expect("send writer id");
                let retired = slot.store(Arc::new(ThreadTracked { drops }));
                drop(retired);
            });
            (handle, rx.recv().expect("writer id"))
        };
        // The writer is blocked by `held`; once it is released the writer must finish
        // promptly and free the payload on its own thread.
        thread::sleep(Duration::from_millis(40));
        assert!(
            !store.is_finished(),
            "the writer cannot have finished while a guard is alive"
        );
        drop(held);
        store.join().expect("writer");
        assert!(started.elapsed() >= Duration::from_millis(40));

        let observed = drops.lock().expect("drops lock").clone();
        assert_eq!(observed.len(), 1);
        assert_eq!(
            observed[0], writer_id,
            "the retired payload must be freed by the writer thread, not by a reader"
        );
    }
}
