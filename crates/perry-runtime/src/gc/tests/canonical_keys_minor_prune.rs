//! Minor death ordering, including a must-fail trie-size witness. The skip
//! exists only in libtest: no production knob, log, or registry is added.
use super::super::*;
use super::support::*;
use crate::array::ArrayHeader;
use crate::object::canonical_keys::{self, CanonicalKeys, SharedLayout};

crate::perry_thread_local! {
    static SKIP_MINOR_PRUNE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(in crate::gc) fn skip_minor_prune() -> bool {
    SKIP_MINOR_PRUNE.with(|v| v.get())
}

struct SkipPrune;
impl Drop for SkipPrune {
    fn drop(&mut self) {
        SKIP_MINOR_PRUNE.with(|v| v.set(false));
    }
}

fn scanners() {
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
}

unsafe fn object(names: &[&str], value: f64) -> *mut crate::object::ObjectHeader {
    // Use the ordinary JSON-record birth, whose descriptor is weak. Setter
    // growth installs transition-cache carriers, which legitimately keep
    // lists alive until the full-trace carrier census; those lists are not
    // dead minor owners and cannot witness this prune.
    let keys = list(names);
    assert!(crate::arena::pointer_in_nursery(keys.addr()));
    let obj =
        crate::object::alloc_plain::alloc_plain_record_with_keys(names.len() as u32, keys.view());
    for index in 0..names.len() {
        crate::object::js_object_set_field(obj, index as u32, crate::JSValue::number(value));
    }
    obj
}

unsafe fn list(names: &[&str]) -> CanonicalKeys {
    let mut raw = crate::array::js_array_alloc(names.len() as u32);
    for name in names {
        raw = crate::array::js_array_push(
            raw,
            crate::JSValue::try_short_string(name.as_bytes()).unwrap_or_else(|| {
                crate::JSValue::string_ptr(crate::string::js_string_from_bytes(
                    name.as_ptr(),
                    name.len() as u32,
                ))
            }),
        );
    }
    canonical_keys::canonicalize(&SharedLayout::shape_cache_entry(), raw, names.len() as u32)
}

unsafe fn assert_names(arr: *const ArrayHeader, names: &[&str]) {
    for (i, name) in names.iter().enumerate() {
        let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
        assert_eq!(
            crate::string::js_string_key_bytes(crate::array::js_array_get(arr, i as u32), &mut sso),
            Some(name.as_bytes())
        );
    }
}

#[test]
fn a_minor_prunes_young_lists_in_copying_and_fallback_paths() {
    for copying in [true, false] {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force = ForcedEvacuationTestGuard::on();
        scanners();
        canonical_keys::reset_for_test();
        let dead = unsafe { list(&["dead", "young"]) };
        assert!(crate::arena::pointer_in_nursery(dead.addr()));
        assert_eq!(canonical_keys::published_lists_for_test().len(), 1);
        if !copying {
            js_gc_write_barriers_emitted(0);
        }
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(matches!(trace.collection_kind, GcCollectionKind::Minor));
        assert_eq!(trace.copying_nursery.eligible, copying);
        assert_eq!(canonical_keys::published_lists_for_test().len(), 0);
        canonical_keys::reset_for_test();
    }
}

#[test]
fn a_list_promoted_in_this_minor_remains_interned() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _age = crate::gc::tenuring::set_survivals_for_test(1);
    scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    let before = unsafe { list(&["live", "keys"]) };
    assert!(crate::arena::pointer_in_nursery(before.addr()));
    let root = scope.root_raw_mut_ptr(before.as_ptr());
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.promoted_objects > 0);
    let after = root.with_const_ptr(|p: *const ArrayHeader| p as usize);
    assert_ne!(before.addr(), after);
    assert!(crate::arena::pointer_in_old_gen(after));
    unsafe {
        assert_eq!(list(&["live", "keys"]).addr(), after);
        assert_names(after as *const _, &["live", "keys"]);
    }
    gc_collect_minor();
    assert_eq!(unsafe { list(&["live", "keys"]).addr() }, after);
    canonical_keys::reset_for_test();
}

#[test]
fn a_live_child_rewitnesses_its_dead_parent_before_the_flip() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    let parent = unsafe { list(&["parent"]) };
    // Whole-list publication allocates a separate backing for the child.
    let child = unsafe { list(&["parent", "child"]) };
    assert_ne!(parent.addr(), child.addr());
    let child = scope.root_raw_mut_ptr(child.as_ptr());
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects > 0);
    let after = child.with_const_ptr(|p: *const ArrayHeader| p as usize);
    assert!(!canonical_keys::published_lists_for_test()
        .iter()
        .any(|&p| p as usize == parent.addr()));
    unsafe {
        assert_eq!(list(&["parent", "child"]).addr(), after);
        let new_parent = list(&["parent"]);
        assert_names(new_parent.as_ptr(), &["parent"]);
        assert_eq!(list(&["parent", "child"]).addr(), after);
    }
    canonical_keys::reset_for_test();
}

fn churn(sabotaged: bool) -> bool {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _age =
        crate::gc::tenuring::set_survivals_for_test(crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX);
    scanners();
    canonical_keys::reset_for_test();
    let scope = RuntimeHandleScope::new();
    let sentinel = scope.root_raw_mut_ptr(unsafe { object(&["keep", "again"], 42.0) });
    let mut bounded = true;
    for round in 0..4 {
        let minted_before = canonical_keys::canonical_stats().1;
        for i in 0..2048 {
            let name = format!("shape_key_{i}");
            let obj = unsafe { object(&[&name, "again"], (round * 2048 + i) as f64) };
            unsafe {
                let keys = crate::object::object_keys(obj);
                assert_eq!(keys.count(), 2);
                assert_names(keys.arr(), &[&name, "again"]);
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                assert_eq!(
                    crate::object::js_object_get_field_by_name(obj, key).as_number(),
                    (round * 2048 + i) as f64
                );
            }
        }
        assert!(canonical_keys::canonical_stats().1 > minted_before,
            "re-interning after pruning must mint new nodes, even when array addresses are recycled");
        assert_eq!(canonical_keys::published_lists_for_test().len(), 2049);
        let trace = {
            let _skip = sabotaged.then(|| {
                SKIP_MINOR_PRUNE.with(|v| v.set(true));
                SkipPrune
            });
            collect_minor_trace(GcTriggerKind::Direct)
        };
        assert!(matches!(trace.collection_kind, GcCollectionKind::Minor));
        assert!(trace.copying_nursery.eligible);
        if round == 0 {
            assert!(
                trace.copying_nursery.copied_objects > 0,
                "the witness must exercise evacuation before its sentinel ages into old generation"
            );
        }
        let remaining = canonical_keys::published_lists_for_test().len();
        let (edges, addresses, bytes) = canonical_keys::canonical_trie_stats();
        eprintln!("minor trie churn: round={round} sabotage={sabotaged} published={remaining} edges={edges} addresses={addresses} reserved_payload_bytes={bytes}");
        bounded &= remaining == 1 && edges == 2 && addresses == 1;
        if sabotaged {
            break; // Never dereference the deliberately stale edges.
        }
        sentinel.with_const_ptr(|p| unsafe {
            assert_names(crate::object::object_keys(p).arr(), &["keep", "again"])
        });
    }
    canonical_keys::reset_for_test();
    bounded
}

#[test]
fn reinterning_after_repeated_minor_prunes_returns_correct_new_lists() {
    assert!(
        churn(false),
        "dead shape lists must leave the trie at every minor"
    );
}

#[test]
fn skipping_minor_prune_turns_the_trie_size_witness_red() {
    assert!(
        !churn(true),
        "the witness must detect a skipped minor prune"
    );
}
