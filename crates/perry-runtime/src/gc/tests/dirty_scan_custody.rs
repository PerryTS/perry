//! Dirty scanning and re-remembering share the descriptor's custody verdict.
use super::super::*;
use super::support::*;

#[derive(Clone, Copy, PartialEq)]
enum Verdict {
    /// The dirty scan's own custody verdict (parent + descriptor).
    Scan,
    /// Generation-only custody: external iff the slot is not old-gen.
    GenerationOnly,
    /// Descriptor-blind custody: every slot needs an owner entry.
    AllExternal,
}

fn custody_disagreements(mode: Verdict) -> usize {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    unsafe {
        let (old, _) = alloc_old_test_array(8);
        let captures = vec![crate::value::TAG_UNDEFINED; LARGE_OBJECT_THRESHOLD_BYTES / 8 + 64];
        let malloc = crate::closure::js_closure_alloc_init(
            std::ptr::null(),
            captures.len() as u32,
            captures.as_ptr(),
        );
        layout_init_unknown_fresh(malloc as *mut u8);
        // An OLD lazy array: its element slots are in-object, enumerated by
        // the owner's LazyArray descriptor (element range), never external.
        let lazy = crate::arena::arena_alloc_gc_old(
            std::mem::size_of::<crate::json_tape::LazyArrayHeader>() + 8,
            8,
            GC_TYPE_LAZY_ARRAY,
        )
        .cast::<crate::json_tape::LazyArrayHeader>();
        std::ptr::write_bytes(lazy, 0, 1);
        (*lazy).magic = crate::json_tape::LAZY_ARRAY_MAGIC;
        (*lazy).cached_length = 1;
        (*lazy).capacity = 1;
        crate::json_tape::lazy_slot(lazy, 0).write(0);

        let mut disagreements = 0;
        for (user, expected_external) in [
            (old.cast::<u8>(), false),
            (malloc as *mut u8, true),
            (lazy.cast::<u8>(), false),
        ] {
            let header = header_from_user_ptr(user);
            let mut pages = crate::fast_hash::new_ptr_hash_set();
            visit_gc_rewrite_slots::<{ cfg!(test) }>(header, |slot| {
                pages.insert(crate::arena::generation_page_for_addr(slot.slot as usize));
            });
            let mut slots = 0;
            barrier::scan_dirty_object_slots::<{ cfg!(test) }>(
                header,
                &pages,
                &mut Default::default(),
                &mut |slot, external, _| {
                    slots += 1;
                    let verdict = match mode {
                        Verdict::Scan => external,
                        Verdict::GenerationOnly => slot.external(),
                        Verdict::AllExternal => true,
                    };
                    disagreements += usize::from(verdict != expected_external);
                },
            );
            assert!(slots > 0, "each parent must exercise descriptor custody");
        }
        disagreements
    }
}

#[test]
fn dirty_scan_uses_parent_and_descriptor_custody() {
    assert_eq!(custody_disagreements(Verdict::Scan), 0);
}

/// The lazy JSON array's element cache was the one OLD separate buffer a
/// descriptor named, the case where generation-only custody was wrong. Its
/// slots are in-object now, so on these parents the two verdicts agree; a
/// reintroduced old side buffer makes this fail.
#[test]
fn generation_only_custody_agrees_once_lazy_slots_are_in_object() {
    assert_eq!(custody_disagreements(Verdict::GenerationOnly), 0);
}

/// Negative control: the old in-object parents (the array and the lazy array)
/// are described by their page, so a verdict that ignores parent custody must
/// be caught.
#[test]
fn negative_control_descriptor_blind_custody_misses_old_in_object_slots() {
    assert!(custody_disagreements(Verdict::AllExternal) > 0);
}
