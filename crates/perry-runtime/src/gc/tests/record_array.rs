//! Value-record arrays under the collector: the head's one edge is traced and
//! rewritten, the buffer is a leaf, and a growth into an old head is
//! remembered.
use super::support::*;
use crate::gc::*;
use crate::record_array::*;

const WORDS: usize = 14;

fn record(i: u32) -> [u64; WORDS] {
    let mut rec = [0u64; WORDS];
    for (w, word) in rec.iter_mut().enumerate() {
        *word = match (i as usize + w) % 6 {
            0 => (i as f64 + w as f64 * 0.5).to_bits(),
            1 => crate::value::TAG_UNDEFINED,
            2 => crate::value::TAG_TRUE,
            3 => crate::value::TAG_NULL,
            4 => (-0.0f64).to_bits(),
            _ => crate::value::TAG_FALSE,
        };
    }
    rec
}

fn rooted() -> f64 {
    f64::from_bits(js_shadow_slot_get(0))
}

fn head() -> *mut RecordArrayHeader {
    (js_shadow_slot_get(0) & POINTER_MASK) as *mut RecordArrayHeader
}

fn assert_records(count: u32) {
    let arr = rooted();
    assert_eq!(js_record_array_length(arr), count as f64);
    for i in 0..count {
        let mut out = [0u64; WORDS];
        assert_eq!(
            js_record_array_get(arr, i as f64, out.as_mut_ptr()).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(out, record(i), "record {i}");
    }
}

fn collect_full() {
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    });
}

#[test]
fn record_array_head_exposes_exactly_its_buffer_edge() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert!(!crate::gc_type_is_array!(GC_TYPE_RECORD_ARRAY));
    assert!(!crate::gc_type_is_array!(GC_TYPE_RECORD_ELEMS));
    let arr = js_record_array_new(WORDS as u32, 3);
    js_shadow_slot_set(0, arr.to_bits());
    js_record_array_push(arr, record(0).as_ptr());
    let header = head();
    unsafe {
        let gc = &*header_from_trusted_user_ptr(header.cast::<u8>());
        assert_eq!(gc.obj_type, GC_TYPE_RECORD_ARRAY);
        let buffer = (*header).elems;
        let gc = &*header_from_trusted_user_ptr(buffer.cast::<u8>());
        assert_eq!(gc.obj_type, GC_TYPE_RECORD_ELEMS);
        assert_eq!(
            test_gc_rewrite_slot_addresses(header as usize).unwrap(),
            vec![std::ptr::addr_of!((*header).elems) as usize]
        );
        assert!(test_gc_rewrite_slot_addresses(buffer as usize)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn record_array_head_and_buffer_move_together_in_a_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_record_array_new(WORDS as u32, 3);
    js_shadow_slot_set(0, arr.to_bits());
    for i in 0..5 {
        js_record_array_push(rooted(), record(i).as_ptr());
    }
    let before = head();
    let buffer_before = unsafe { (*before).elems };
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects >= 2);
    let after = head();
    assert_ne!(after, before, "the head moved");
    let buffer_after = unsafe { (*after).elems };
    assert_ne!(buffer_after, buffer_before, "the edge was rewritten");
    assert!(build_valid_pointer_set().contains(&(buffer_after as usize)));
    assert_records(5);
}

#[test]
fn record_array_old_head_growing_into_a_young_buffer_is_remembered() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_record_array_new(WORDS as u32, 3);
    js_shadow_slot_set(0, arr.to_bits());
    for i in 0..4 {
        js_record_array_push(rooted(), record(i).as_ptr());
    }
    for _ in 0..4 {
        if crate::arena::pointer_in_old_gen(head() as usize) {
            break;
        }
        gc_collect_minor();
    }
    let header = head();
    assert!(
        crate::arena::pointer_in_old_gen(header as usize),
        "fixture: the head was promoted"
    );
    assert_records(4);
    unsafe {
        assert_eq!((*header).capacity, 4);
    }
    reset_remembered_set();

    // The fifth push grows: a nursery buffer installed in the old head.
    js_record_array_push(rooted(), record(4).as_ptr());
    assert_eq!(head(), header, "an old head stays put");
    let young_buffer = unsafe { (*header).elems } as usize;
    assert!(crate::arena::pointer_in_nursery(young_buffer));
    assert!(
        remembered_set_size() > 0,
        "the buffer install must remember the old head"
    );
    // Churn, so a buffer the minor missed would be overwritten.
    for _ in 0..64 {
        young_leaf();
    }
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects >= 1);
    let buffer = unsafe { (*header).elems } as usize;
    assert_ne!(buffer, young_buffer, "the old head's edge was rewritten");
    assert!(build_valid_pointer_set().contains(&buffer));
    for _ in 0..64 {
        young_leaf();
    }
    assert_records(5);
    collect_full();
    assert_records(5);
}

#[test]
fn record_array_grows_to_ten_thousand_records_across_collections() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_record_array_new(WORDS as u32, 3);
    js_shadow_slot_set(0, arr.to_bits());
    const COUNT: u32 = 10_000;
    let mut grown_while_old = 0;
    for i in 0..COUNT {
        let old = crate::arena::pointer_in_old_gen(head() as usize);
        let capacity = unsafe { (*head()).capacity };
        assert_eq!(
            js_record_array_push(rooted(), record(i).as_ptr()),
            (i + 1) as f64
        );
        if old && unsafe { (*head()).capacity } != capacity {
            grown_while_old += 1;
        }
        if i % 61 == 0 {
            young_leaf();
            gc_collect_minor();
        }
        if i % 1777 == 3 {
            collect_full();
            assert_records(i + 1);
        }
    }
    assert!(
        grown_while_old >= 8,
        "the old head grew {grown_while_old} times"
    );
    assert_eq!(unsafe { (*head()).capacity }, 16_384);
    assert_records(COUNT);
    gc_collect_minor();
    collect_full();
    assert_records(COUNT);

    // Pop back down and refill: the buffer is kept.
    let mut out = [0u64; WORDS];
    for i in (5_000..COUNT).rev() {
        assert_eq!(
            js_record_array_pop(rooted(), out.as_mut_ptr()).to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(out, record(i));
    }
    gc_collect_minor();
    assert_records(5_000);

    // Unreachable: the head and its buffer are garbage for both collectors.
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    gc_collect_minor();
    collect_full();
    for _ in 0..256 {
        young_leaf();
    }
    gc_collect_minor();
    collect_full();
}

#[test]
fn record_array_push_survives_a_collection_inside_its_growth() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = js_record_array_new(WORDS as u32, 3);
    js_shadow_slot_set(0, arr.to_bits());
    let mut moved_in_push = 0;
    for i in 0..3_000u32 {
        let full = unsafe { (*head()).length == (*head()).capacity };
        let before = head();
        if full {
            triggers.make_arena_trigger_due();
        }
        // `rooted()` is the caller's copy of `arr`: stale once the head moves.
        js_record_array_push(rooted(), record(i).as_ptr());
        if head() != before {
            moved_in_push += 1;
        }
    }
    eprintln!("record array head moved inside push {moved_in_push} times");
    assert_records(3_000);
}
