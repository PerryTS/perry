use super::support::*;

fn pointer_bits() -> u64 {
    let child = crate::string::js_string_from_bytes(b"residue-child".as_ptr(), 13);
    string_bits(child as usize)
}

fn closure_with_captures(slot_count: usize, pointer_slots: usize) -> usize {
    let pointer = pointer_bits();
    let mut captures = vec![1.0f64.to_bits(); slot_count];
    captures[..pointer_slots].fill(pointer);
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), slot_count as u32);
    unsafe {
        let slots = crate::closure::closure_capture_slots_mut(closure);
        std::ptr::copy_nonoverlapping(captures.as_ptr(), slots, slot_count);
        crate::gc::layout_init_from_slots(closure as *mut u8, slots, slot_count);
    }
    closure as usize
}

/// The requested marginal-histogram fixture. Swapping the `8..=15` and
/// `16..=31` bounds makes the 20-capture assertion fail.
#[test]
fn layout_residue_histogram_counts_by_kind_and_bucket() {
    let _gc = CopyingNurseryTestGuard::new(0);
    let _diag = crate::hot_diag::LayoutDiagTestGuard::force(true);

    let closure5 = closure_with_captures(5, 1);
    let closure20 = closure_with_captures(20, 10);

    let object70 = crate::object::js_object_alloc(0, 70);
    let pointer = pointer_bits();
    unsafe {
        let fields = (object70 as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *mut u64;
        for slot in 0..70 {
            fields.add(slot).write(if slot < 60 {
                pointer
            } else {
                (slot as f64).to_bits()
            });
        }
        crate::object::rebuild_object_field_layout(object70, 70);
    }

    crate::gc::layout_tables::test_reset_layout_residue_histogram_entries();
    crate::gc::layout_tables::prune_dead_per_object_layout_owners(&|_| false);

    let residue = crate::hot_diag::LayoutDiagTestGuard::residue();
    assert_eq!(residue.keys, 3, "{residue:?}");
    assert_eq!(residue.closure, 2, "{residue:?}");
    assert_eq!(residue.object, 1, "{residue:?}");
    assert_eq!(residue.array, 0, "{residue:?}");
    assert_eq!(residue.other, 0, "{residue:?}");
    assert_eq!(residue.slots, [1, 0, 1, 0, 1, 0], "{residue:?}");
    assert_eq!(residue.pointer_share, [1, 1, 0, 1], "{residue:?}");
    assert_eq!(residue.space, [3, 0, 0], "{residue:?}");
    assert_eq!(residue.est_tag_checks_saved_per_trace, 24, "{residue:?}");
    assert_eq!(
        crate::gc::layout_tables::test_layout_residue_histogram_entries(),
        3
    );

    let output = crate::hot_diag::LayoutDiagTestGuard::output();
    assert!(output.contains("[layout-diag] residue keys=3 closure=2 object=1 array=0 other=0"));
    assert!(output.contains("slots{4-7=1 8-15=0 16-31=1 32-63=0 64-255=1 256+=0}"));
    assert!(output.contains("ptr_share{q1=1 q2=1 q3=0 q4=1}"));
    assert!(output.contains("space{nursery=3 old=0 malloc=0}"));
    assert!(output.contains("inserts_since{birth=2 rebuild=1 store=0}"));
    assert!(output.contains("[layout-diag] price per_key_prune_ns="));
    assert!(output.contains("est_tag_checks_saved_per_trace=24"));

    for owner in [closure5, closure20, object70 as usize] {
        crate::gc::layout_clear_for_ptr(owner);
    }
}

/// Dropping the single `layout_on()` gate in either prune makes the test-only
/// entry counter non-zero, even though the output sink remains unarmed.
#[test]
fn layout_residue_histogram_is_silent_when_unarmed() {
    let _gc = CopyingNurseryTestGuard::new(0);
    let _diag = crate::hot_diag::LayoutDiagTestGuard::force(false);
    let closure = closure_with_captures(5, 1);

    crate::gc::layout_tables::test_reset_layout_residue_histogram_entries();
    crate::gc::layout_tables::prune_dead_per_object_layout_owners(&|_| false);

    assert!(
        crate::hot_diag::LayoutDiagTestGuard::output().is_empty(),
        "an unarmed prune must emit no residue line"
    );
    assert_eq!(
        crate::gc::layout_tables::test_layout_residue_histogram_entries(),
        0,
        "the histogram entry loop must not run while the sink is off"
    );
    crate::gc::layout_clear_for_ptr(closure);
}
