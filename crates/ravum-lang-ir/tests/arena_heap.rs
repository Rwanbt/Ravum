//! STD-21.3..21.5 on the IR heap (path C), with the real limits: a slot is
//! reused until its generation reaches `2^24 - 1`, then retired forever; a
//! stale or foreign handle is never another element.

use ravum_lang_ir::heap::{unpack_handle, Heap, ARENA_GEN_MAX};
use ravum_lang_ir::ir::HeapOp;

#[test]
fn a_slot_is_retired_at_the_last_generation() {
    let mut heap = Heap::default();
    let a = heap.op(HeapOp::ArenaNew, 0, 0).unwrap();
    let b = heap.op(HeapOp::ArenaNew, 0, 0).unwrap();
    let first = heap.op(HeapOp::ArenaInsert, a, 7).unwrap();
    assert_eq!(unpack_handle(first), (1, 0, 0));
    // A handle of `a` is foreign to `b`.
    assert_eq!(heap.map_get(b, first).unwrap(), (false, 0));
    let mut last = first;
    for g in 0..ARENA_GEN_MAX {
        assert_eq!(
            heap.map_remove(a, last).unwrap(),
            (true, if g == 0 { 7 } else { g })
        );
        // Stale: absent, and removing again changes nothing.
        assert_eq!(heap.map_get(a, last).unwrap(), (false, 0));
        assert_eq!(heap.map_remove(a, last).unwrap(), (false, 0));
        if g + 1 < ARENA_GEN_MAX {
            last = heap.op(HeapOp::ArenaInsert, a, g + 1).unwrap();
            assert_eq!(
                unpack_handle(last),
                (1, 0, g + 1),
                "slot 0 reused with the next generation"
            );
        }
    }
    // Slot 0 reached the last generation: retired, the next insert takes slot 1.
    let next = heap.op(HeapOp::ArenaInsert, a, 9).unwrap();
    assert_eq!(unpack_handle(next), (1, 1, 0));
    assert_eq!(heap.map_get(a, first).unwrap(), (false, 0));
    assert_eq!(heap.op(HeapOp::MapLen, a, 0).unwrap(), 1);
}

/// Replacing the value of a present `Map` key allocates nothing: 20 million
/// replacements stay within the heap limit (before, each one charged 24
/// bytes and the run faulted with `heap limit`, unlike the rustc path).
#[test]
fn replacing_a_map_value_does_not_charge_the_heap() {
    let mut heap = Heap::default();
    let m = heap.op(HeapOp::MapNew, 1, 0).unwrap();
    for i in 0..20_000_000u64 {
        heap.map_insert(m, 5, i).unwrap();
    }
    assert_eq!(heap.map_get(m, 5).unwrap(), (true, 19_999_999));
}
