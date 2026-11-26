use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;
use ui_core::surfaces::*;

/// Compte les allocations du thread courant (pour prouver INV-05 côté push).
struct Counting;
thread_local!(static ALLOCS: Cell<u64> = const { Cell::new(0) });
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn allocs() -> u64 {
    ALLOCS.with(|c| c.get())
}

#[test]
fn ring_push_never_allocates_and_drops_when_full() {
    let r = Ring::new(4);
    let before = allocs();
    for i in 0..10 {
        r.push(i as f32);
    }
    assert_eq!(allocs(), before, "push allocated");
    assert_eq!(r.dropped(), 6);
    assert_eq!(
        (0..5).map(|_| r.pop()).collect::<Vec<_>>(),
        vec![Some(0.0), Some(1.0), Some(2.0), Some(3.0), None]
    );
}

#[test]
fn ring_crosses_threads_in_order() {
    const N: u32 = 200_000;
    let r = Arc::new(Ring::new(1024));
    let p = Arc::clone(&r);
    let producer = std::thread::spawn(move || {
        let before = allocs();
        let mut i = 0;
        while i < N {
            if p.push(i as f32) {
                i += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        allocs() - before
    });
    let mut next = 0u32;
    while next < N {
        if let Some(x) = r.pop() {
            assert_eq!(x, next as f32);
            next += 1;
        }
    }
    assert_eq!(producer.join().unwrap(), 0, "the audio side allocated");
}

#[test]
fn peaks_match_a_direct_scan_at_every_zoom() {
    let mut seed = 0x9e37_79b9u32;
    let s: Vec<f32> = (0..10_007)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect();
    let p = Peaks::build(&s);
    let direct = |a: usize, b: usize| {
        s[a..b]
            .iter()
            .fold((f32::MAX, f32::MIN), |m, &x| (m.0.min(x), m.1.max(x)))
    };
    for (a, b) in [
        (0, 1),
        (0, 10_007),
        (3, 4_099),
        (1_024, 2_048),
        (9_999, 10_007),
        (5_000, 5_001),
    ] {
        assert_eq!(p.range(a, b), Some(direct(a, b)), "[{a}, {b})");
    }
    assert_eq!(p.range(10, 10), None);
    assert_eq!(p.range(10_000, 20_000), Some(direct(10_000, 10_007)));
    for width in [1, 7, 640, 3_000] {
        let cols = p.columns(0, s.len(), width);
        assert_eq!(cols.len(), width);
        let all = cols
            .iter()
            .flatten()
            .copied()
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
            .unwrap();
        assert_eq!(
            all,
            direct(0, s.len()),
            "width {width}: columns cover every sample"
        );
    }
}

#[test]
fn timeline_zoom_keeps_the_cursor_time_fixed() {
    let mut t = Timeline {
        start: 10.0,
        px_per_s: 100.0,
        min_px_per_s: 1.0,
        max_px_per_s: 10_000.0,
    };
    let at = t.to_time(250.0);
    t.zoom(250.0, 4.0);
    assert!((t.to_time(250.0) - at).abs() < 1e-9);
    assert_eq!(t.px_per_s, 400.0);
    t.zoom(250.0, 1e9);
    assert_eq!(t.px_per_s, 10_000.0);
    let before = t;
    t.zoom(0.0, f64::NAN);
    t.zoom(0.0, -2.0);
    assert_eq!(t, before);
    t.zoom(1_000.0, 1e-9);
    assert_eq!((t.px_per_s, t.start), (1.0, 0.0), "start clamped at 0");
    assert!((t.to_px(t.to_time(123.0)) - 123.0).abs() < 1e-9);
}
