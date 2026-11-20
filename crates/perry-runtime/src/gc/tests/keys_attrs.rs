//! Charter step 3: a keys array's attributes array hangs off its
//! named-properties reserve word (`object/key_attrs.rs`). The collector already
//! emits that word as a fixed child slot of every flagged array; this pins
//! that canonical attributes stay old and stationary, and an explicitly
//! installed young attributes child still moves and remains readable.
//!
//! Fault injection this catches: a keys-list allocator that forgets
//! `GC_ARRAY_KEY_ATTRS` (the reserve word is then never visited, the
//! attributes array dies in the nursery, and the entries read back as
//! whatever reuses its storage) — `attributes_survive_a_moving_minor`.

use super::super::*;
use super::support::{collect_minor_trace, CopyingNurseryTestGuard, GcTriggerThresholdTestGuard};
use crate::object::canonical_keys::{extend_key_with_entry, CanonicalKeys, SharedLayout};
use crate::object::key_attrs::{self, ENTRY_ACCESSOR, ENTRY_HAS_GET, ENTRY_NON_ENUMERABLE};

#[test]
fn attributes_survive_a_moving_minor() {
    for canonical_attrs in [true, false] {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        super::support::register_runtime_handle_root_scanner_for_tests();
        crate::object::canonical_keys::reset_for_test();
        let proof = SharedLayout::shape_cache_entry();
        let entries = [0u8, ENTRY_NON_ENUMERABLE, ENTRY_ACCESSOR | ENTRY_HAS_GET];
        let scope = RuntimeHandleScope::new();
        let list = scope.root_raw_mut_ptr::<crate::ArrayHeader>(std::ptr::null_mut());
        let mut cur = CanonicalKeys::EMPTY;
        for (i, &e) in entries.iter().enumerate() {
            let name = format!("ka_moving_{i}");
            let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            // `extend_key_with_entry` roots its operands across its allocation.
            let next = list.with_mut_ptr(|arr: *mut crate::ArrayHeader| unsafe {
                extend_key_with_entry(&proof, cur.rooted_at(arr), k, e)
            });
            list.set_raw_mut_ptr(next.as_ptr());
            cur = next;
        }
        if !canonical_attrs {
            // Keep the relocation control: an old key list with an owned young
            // attributes child still depends on tracing its reserve word.
            let (young, keys) =
                list.across_mut::<crate::ArrayHeader, _>(|| unsafe { key_attrs::alloc_attrs(3) });
            unsafe {
                key_attrs::copy_entries(keys, 0, young, 3);
                key_attrs::attach_attrs(keys, young);
            }
        }
        let (before_keys, before_attrs) = list.with_mut_ptr(|keys: *mut crate::ArrayHeader| {
            (keys as usize, unsafe { key_attrs::keys_attrs(keys) }
                as usize)
        });
        assert_ne!(
            before_attrs, 0,
            "premise: the list carries an attributes array"
        );
        assert!(
            if canonical_attrs {
                crate::arena::pointer_in_old_gen(before_attrs)
            } else {
                crate::arena::pointer_in_nursery(before_attrs)
            },
            "premise: canonical attributes are old, the owned control is young"
        );
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(
            trace.copying_nursery.copied_objects + trace.copying_nursery.promoted_objects > 0,
            "premise: the minor relocated children"
        );
        if !canonical_attrs {
            assert!(
                trace.copying_nursery.copied_objects > 0,
                "the young attributes control copies"
            );
        }
        list.with_mut_ptr(|after_keys: *mut crate::ArrayHeader| {
            let after_attrs = unsafe { key_attrs::keys_attrs(after_keys) } as usize;
            assert_eq!(
                after_keys as usize, before_keys,
                "I3: the old keys backing is stationary"
            );
            if canonical_attrs {
                assert_eq!(after_attrs, before_attrs, "I3 attributes stay stationary");
            } else {
                assert_ne!(after_attrs, before_attrs, "the owned control must move");
            }
            for (i, &e) in entries.iter().enumerate() {
                assert_eq!(
                    unsafe { key_attrs::keys_entry(after_keys, i as u32) },
                    e,
                    "entry {i} after the move"
                );
            }
            assert_eq!(
                unsafe { key_attrs::keys_summary(after_keys, 3) },
                key_attrs::SUMMARY_NON_ENUMERABLE | key_attrs::SUMMARY_ACCESSOR
            );
        });
    }
}

#[test]
fn stationary_attributes_are_reclaimable_with_their_unreachable_keys() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = super::support::ConservativeScanDisabledGuard::new();
    super::support::register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let keys = unsafe { key_attrs::alloc_canonical_key_list(4, true, true) };
    let attrs = unsafe { key_attrs::keys_attrs(keys) };
    assert!(crate::arena::pointer_in_old_gen(attrs as usize));
    let _root = scope.root_raw_mut_ptr(keys);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let present = || {
        let mut found = [false, false];
        crate::arena::old_arena_walk_objects(|header| {
            let user = header as usize + GC_HEADER_SIZE;
            found[0] |= user == keys as usize;
            found[1] |= user == attrs as usize;
        });
        found
    };
    assert_eq!(
        present(),
        [true, true],
        "the rooted pair must survive a full trace"
    );
    drop(scope);
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    // Arena page membership can outlive cells, and whole-block retirement
    // need not reuse the same address next. The allocator's live object walk
    // proves removal without reading either freed candidate header.
    assert_eq!(
        present(),
        [false, false],
        "movement pin must not pin liveness"
    );
}
