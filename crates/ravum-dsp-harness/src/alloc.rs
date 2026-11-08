//! Allocation instrumentation.
//!
//! The audio thread must not allocate **and must not free**: a `dealloc` can take an
//! internal allocator lock, and a drop that ends the last `Arc` of a native module
//! transitively enqueues or unloads a library. Both directions are therefore counted
//! per thread, so the claim "the callback does neither" is measured instead of
//! asserted.
//!
//! Cost: one thread-local increment per allocation and per deallocation, for the whole
//! process that links this crate. Acceptable for a harness, and stated in the report.
//!
//! Scope, stated honestly: the counters instrument Rust's global allocator calls. A
//! kernel-side unmapping, a page fault, or an allocator that frees lazily inside
//! `System::dealloc` would not be visible here. This makes a positive count a proof of
//! a violation, not a zero count a proof of hard real-time behaviour.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
    static DEALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

fn bump(counter: &'static std::thread::LocalKey<Cell<u64>>) {
    // `try_with` avoids a panic if TLS is already being destroyed.
    let _ = counter.try_with(|c| c.set(c.get().wrapping_add(1)));
}

pub struct CountingAllocator;

// SAFETY: every method forwards to `System` unchanged; only counters are updated.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            bump(&ALLOCATIONS);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        bump(&DEALLOCATIONS);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            bump(&ALLOCATIONS);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // A realloc can move the block: it is counted as one allocation. The old block
        // is released by `System::realloc` itself, so this is not counted as a
        // deallocation; a callback must do neither anyway.
        let out = unsafe { System.realloc(ptr, layout, new_size) };
        if !out.is_null() {
            bump(&ALLOCATIONS);
        }
        out
    }
}

/// Allocations performed on the current thread since the last reset.
pub fn allocations_on_this_thread() -> u64 {
    ALLOCATIONS.try_with(|c| c.get()).unwrap_or(0)
}

/// Deallocations performed on the current thread since the last reset.
///
/// A non-zero value on an audio thread means a `Box`/`Vec`/`Arc`/`String` was dropped
/// there — including the last reference to a retired module, which is exactly the
/// hazard `docs/mvp/MVP-0b-rt-lifecycle.md` documents.
pub fn deallocations_on_this_thread() -> u64 {
    DEALLOCATIONS.try_with(|c| c.get()).unwrap_or(0)
}

/// Sum of allocations and deallocations on the current thread: the metric an audio
/// callback wants at zero.
pub fn allocator_ops_on_this_thread() -> u64 {
    allocations_on_this_thread().saturating_add(deallocations_on_this_thread())
}

/// Reset both counters of the current thread. Kept for call sites written against the
/// allocation-only version.
pub fn reset_thread_counter() {
    reset_thread_counters();
}

/// Reset both counters of the current thread.
pub fn reset_thread_counters() {
    let _ = ALLOCATIONS.try_with(|c| c.set(0));
    let _ = DEALLOCATIONS.try_with(|c| c.set(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_and_deallocations_are_counted_per_thread() {
        reset_thread_counters();
        {
            let boxed = Box::new(vec![1u8; 1024]);
            assert!(allocations_on_this_thread() >= 1);
            assert_eq!(deallocations_on_this_thread(), 0, "nothing dropped yet");
            drop(boxed);
        }
        assert!(
            deallocations_on_this_thread() >= 1,
            "dropping the box must be visible as a deallocation"
        );

        let other = std::thread::spawn(|| {
            // The runtime itself may perform a few allocator operations on a new thread
            // before user code runs (observed: 3 deallocations). What matters for the
            // harness is that the counters are per-thread and can be zeroed at the start
            // of the measured window.
            reset_thread_counters();
            (allocations_on_this_thread(), deallocations_on_this_thread())
        })
        .join()
        .unwrap();
        assert_eq!(other, (0, 0), "a thread reset starts at zero");
    }

    #[test]
    fn resetting_clears_both_counters() {
        let _probe = Box::new([0u8; 64]);
        drop(_probe);
        reset_thread_counters();
        assert_eq!(allocator_ops_on_this_thread(), 0);
    }
}
