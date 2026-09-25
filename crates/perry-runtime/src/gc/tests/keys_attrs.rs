//! Charter step 3: a keys array's attributes array hangs off its
//! named-properties reserve word (`object/key_attrs.rs`). The collector already
//! emits that word as a fixed child slot of every flagged array; this pins
//! that a real copying minor moves BOTH the keys backing and its attributes
//! array and leaves the entries readable through the moved keys.
//!
//! Fault injection this catches: a keys-list allocator that forgets
//! `GC_ARRAY_NAMED_PROPS` (the reserve word is then never visited, the
//! attributes array dies in the nursery, and the entries read back as
//! whatever reuses its storage) — `attributes_survive_a_moving_minor`.

use super::super::*;
use super::support::{collect_minor_trace, CopyingNurseryTestGuard, GcTriggerThresholdTestGuard};
use crate::object::canonical_keys::{extend_key_with_entry, CanonicalKeys, SharedLayout};
use crate::object::key_attrs::{self, ENTRY_ACCESSOR, ENTRY_HAS_GET, ENTRY_NON_ENUMERABLE};

#[test]
fn attributes_survive_a_moving_minor() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    super::support::register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    crate::object::canonical_keys::reset_for_test();
    let proof = SharedLayout::shape_cache_entry();
    let entries = [0u8, ENTRY_NON_ENUMERABLE, ENTRY_ACCESSOR | ENTRY_HAS_GET];
    let scope = RuntimeHandleScope::new();
    let list = scope.root_raw_mut_ptr::<crate::ArrayHeader>(std::ptr::null_mut());
    let mut len = 0u32;
    for (i, &e) in entries.iter().enumerate() {
        let name = format!("ka_moving_{i}");
        let k = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        // `extend_key_with_entry` roots its operands across its allocation.
        let next = list.with_mut_ptr(|arr: *mut crate::ArrayHeader| unsafe {
            extend_key_with_entry(&proof, CanonicalKeys::from_rooted(arr, len), k, e)
        });
        list.set_raw_mut_ptr(next.as_ptr());
        len = next.len();
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
        crate::arena::pointer_in_nursery(before_attrs),
        "premise: the attributes array is young, so a copying minor moves it"
    );
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "premise: the minor copied"
    );
    list.with_mut_ptr(|after_keys: *mut crate::ArrayHeader| {
        let after_attrs = unsafe { key_attrs::keys_attrs(after_keys) } as usize;
        assert_ne!(
            after_keys as usize, before_keys,
            "premise: the keys backing moved"
        );
        assert_ne!(
            after_attrs, before_attrs,
            "the attributes array must move with it"
        );
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
