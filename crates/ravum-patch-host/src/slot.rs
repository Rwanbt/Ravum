//! Lock-free publication slot (M3).
//!
//! The audio path must not take a mutex, not allocate, and not wait. This slot gives
//! a reader one atomic increment and no lock, while a writer replaces the pointer.
//!
//! # Why the `readers` counter is enough
//!
//! The only hazard window is between "a reader observed the old pointer" and "the
//! reader has incremented the strong count". The reader announces itself **before**
//! reading the pointer, and the writer, after swapping, waits for that announcement
//! window to drain before releasing the strong reference it just removed. Once a
//! reader has incremented the count, the value cannot be freed under it — the
//! counter, not the announcement, protects actual use.
//!
//! A writer therefore waits only for a two-atomic window (nanoseconds), never for a
//! block to complete. Block-boundary semantics come from the caller, which loads the
//! module once per block and keeps that `Arc` for the whole block.

use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::Arc;

pub struct ArcSlot<T> {
    ptr: AtomicPtr<T>,
    readers: AtomicUsize,
}

impl<T> ArcSlot<T> {
    pub fn new(value: Arc<T>) -> Self {
        Self {
            ptr: AtomicPtr::new(Arc::into_raw(value) as *mut T),
            readers: AtomicUsize::new(0),
        }
    }

    /// Lock-free load. One atomic increment; no allocation; no blocking.
    pub fn load(&self) -> Arc<T> {
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

    /// Publish `value` and return the previous one (still owned, caller drops it).
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
        // SAFETY: the slot owns one strong reference and is being destroyed.
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
    use std::sync::atomic::AtomicUsize;

    struct Tracked {
        id: u64,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for Tracked {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
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
            readers.push(std::thread::spawn(move || {
                let mut last = 0u64;
                for _ in 0..5_000 {
                    let value = slot.load();
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
        let held = slot.load();
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
}
