//! Allocation instrumentation.
//!
//! The audio thread must not allocate. This module installs a counting global
//! allocator so the claim is measurable rather than asserted: counters are
//! thread-local, so a test can isolate the audio thread from everything else.
//!
//! Cost: one thread-local increment per allocation, for the whole process that
//! links this crate. Acceptable for a harness, and stated in the report.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCATIONS: Cell<u64> = const { Cell::new(0) };
}

fn bump() {
    // `try_with` avoids a panic if TLS is already being destroyed.
    let _ = ALLOCATIONS.try_with(|c| c.set(c.get().wrapping_add(1)));
}

pub struct CountingAllocator;

// SAFETY: every method forwards to `System` unchanged; only a counter is updated.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            bump();
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            bump();
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let out = unsafe { System.realloc(ptr, layout, new_size) };
        if !out.is_null() {
            bump();
        }
        out
    }
}

/// Allocations performed on the current thread since the last reset.
pub fn allocations_on_this_thread() -> u64 {
    ALLOCATIONS.try_with(|c| c.get()).unwrap_or(0)
}

pub fn reset_thread_counter() {
    let _ = ALLOCATIONS.try_with(|c| c.set(0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_are_counted_per_thread() {
        reset_thread_counter();
        let boxed = Box::new(vec![1u8; 1024]);
        assert!(allocations_on_this_thread() >= 1);
        drop(boxed);

        let other = std::thread::spawn(|| {
            // A fresh thread starts at zero.
            allocations_on_this_thread()
        })
        .join()
        .unwrap();
        assert_eq!(other, 0);
    }
}
