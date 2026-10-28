//! Owned prototype words survive relocation through each owner's property bag.
//! The owner and prototype movement premises make stale words observable.

use super::super::*;
use super::support::*;

/// Retargeting any array-like latches the process-wide "an array somewhere has
/// a custom `[[Prototype]]`" flag and stands the index fast paths down for the
/// rest of the binary (see `ArrayPrototypeLatchGuard` in `dyn_eval/tests.rs`).
/// Restore what the test found.
struct ArrayPrototypeLatchRestore {
    latch: bool,
    invalidated: u8,
}

impl ArrayPrototypeLatchRestore {
    fn capture() -> Self {
        Self {
            latch: crate::object::prototype_chain::array_static_proto_recorded(),
            invalidated: crate::array::PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl Drop for ArrayPrototypeLatchRestore {
    fn drop(&mut self) {
        crate::object::prototype_chain::test_swap_array_static_proto_recorded(self.latch);
        crate::array::test_swap_array_index_fast_path_invalidated(self.invalidated);
    }
}

/// A small `JSON.parse`-shaped lazy array: small enough that its cluster is
/// born in the nursery (`json_tape::lazy_cluster_is_old`), so a copying minor
/// moves it.
fn nursery_lazy_array(input: &[u8]) -> usize {
    let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
    crate::json_tape::with_built_tape(input, |tape| unsafe {
        crate::json_tape::alloc_lazy_array(
            tape,
            0,
            crate::json_tape::count_array_length(tape, 0),
            text,
        )
    })
    .expect("valid JSON should build a tape") as usize
}

fn obj_type_at(user: usize) -> u8 {
    unsafe { (*header_from_user_ptr(user as *const u8)).obj_type }
}

const MARKER: f64 = 10362.0;

fn marked_prototype() -> usize {
    let proto = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(proto, 0, crate::value::JSValue::number(MARKER));
    proto as usize
}

/// `CopyingNurseryTestGuard` starts with an empty root scanner registry. But
/// `Object.setPrototypeOf` and `Object.getPrototypeOf` on a builtin owner build
/// intrinsics lazily inside the window, and that mints key strings in the
/// thread's string atom table. Without that table's scanner, a copying minor
/// frees the atoms while the table still names them. The next intrinsic build
/// then takes a freed address as a key, and a key list later reads a garbage
/// length from it, up to the end of the 2 MiB block (#12137).
///
/// Register these tables as `gc_init` does: the atom and intern table holds
/// its strings, and the shape table and canonical keys trie follow moved keys
/// arrays. (The same lazy build also memoizes the intrinsic prototypes as raw
/// addresses; every scoped test window registers that cache's scanner itself,
/// see `ScopedRootScannerRegistryGuard::new`.)
fn register_key_table_scanners() {
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
}

/// The claim as reported against #10381: a lazy array's prototype, set through
/// the real `Object.setPrototypeOf` entry, across a copying minor that moves
/// both the array and its prototype.
#[test]
fn test_lazy_array_explicit_prototype_survives_a_copying_minor() {
    let _serialized = crate::array::test_serialize();
    let _feedback = crate::typed_feedback::typed_feedback_test_lock();
    let _latch = ArrayPrototypeLatchRestore::capture();
    // Two rooted values — the lazy array and its prototype — so two shadow
    // slots: a store outside the pushed frame is a silent no-op (#7184).
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_key_table_scanners();

    let lazy = nursery_lazy_array(b"[1,2,3]");
    assert_eq!(
        obj_type_at(lazy),
        GC_TYPE_LAZY_ARRAY,
        "premise: a real lazy array"
    );
    assert!(
        crate::arena::pointer_in_nursery(lazy),
        "premise: the lazy cluster is nursery-born, so a copying minor can move it"
    );
    assert!(crate::gc::gc_type_is_movable(GC_TYPE_LAZY_ARRAY));
    assert!(
        unsafe { crate::object::prototype_chain::meta_capable_object(lazy) }.is_none(),
        "premise: a lazy array has no meta record, so its prototype goes to the owner bag"
    );

    let proto = marked_prototype();
    js_shadow_slot_set(0, ptr_bits(lazy));
    js_shadow_slot_set(1, ptr_bits(proto));

    // The real user-facing entry, not the recorder beneath it.
    crate::object::js_object_set_prototype_of(
        f64::from_bits(ptr_bits(lazy)),
        f64::from_bits(ptr_bits(proto)),
    );
    let lazy = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let proto = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_eq!(
        obj_type_at(lazy),
        GC_TYPE_LAZY_ARRAY,
        "premise: setPrototypeOf left the receiver a lazy array"
    );
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(lazy),
        Some(ptr_bits(proto)),
        "premise: the prototype is recorded in the owned bag under the lazy \
         header's own address"
    );
    assert_eq!(
        crate::object::js_object_get_prototype_of(f64::from_bits(ptr_bits(lazy))).to_bits(),
        ptr_bits(proto),
        "premise: Object.getPrototypeOf resolves it before anything moves"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let lazy_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let proto_after = (js_shadow_slot_get(1) & POINTER_MASK) as usize;
    assert_ne!(
        lazy_after, lazy,
        "the copying minor must actually relocate the lazy header — an unmoved \
         receiver proves nothing"
    );
    assert_ne!(
        proto_after, proto,
        "the prototype must move too, or a stale recorded address would go unseen"
    );
    assert_eq!(obj_type_at(lazy_after), GC_TYPE_LAZY_ARRAY);

    let recorded = crate::object::prototype_chain::object_static_prototype(lazy_after);
    let resolved =
        crate::object::js_object_get_prototype_of(f64::from_bits(ptr_bits(lazy_after))).to_bits();
    // Leave no entry behind for a later test to trip over, whatever the verdict.

    assert_eq!(
        recorded,
        Some(ptr_bits(proto_after)),
        "the owner bag entry must follow the lazy header to its new address and name \
         the prototype at ITS current address"
    );
    assert_eq!(
        resolved,
        ptr_bits(proto_after),
        "Object.getPrototypeOf on the relocated lazy array must still return the \
         prototype it was given"
    );
}

#[test]
fn an_owned_prototype_moves_with_its_array() {
    prototype_edge(false);
}

#[test]
#[ignore = "negative control: erase the relocated owner's prototype slot"]
fn negative_control_objtables_object_prototypes() {
    prototype_edge(true);
}
fn prototype_edge(erase: bool) {
    let _serialized = crate::array::test_serialize();
    let _feedback = crate::typed_feedback::typed_feedback_test_lock();
    let _latch = ArrayPrototypeLatchRestore::capture();
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_key_table_scanners();
    let owner = crate::array::js_array_alloc(8) as usize;
    js_shadow_slot_set(0, ptr_bits(owner));
    let proto = marked_prototype();
    crate::object::prototype_chain::object_set_static_prototype(owner, ptr_bits(proto));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, owner);
    if erase {
        unsafe {
            let bag = crate::object::descriptor_state::descriptor_holder(moved);
            (*(*bag).meta).prototype = 0;
        }
    }
    let bits = crate::object::prototype_chain::object_static_prototype(moved)
        .expect("owner retains prototype");
    assert_ne!((bits & POINTER_MASK) as usize, proto);
    assert_eq!(
        crate::object::js_object_get_field(
            (bits & POINTER_MASK) as *const crate::object::ObjectHeader,
            0
        )
        .bits(),
        MARKER.to_bits()
    );
    let fresh = crate::array::js_array_alloc(8) as usize;
    assert_eq!(
        crate::object::prototype_chain::object_static_prototype(fresh),
        None
    );
}

/// Every movable receiver kind that keeps its prototype in the residual
/// registry, with the prototype held by NOTHING but that entry. One copying
/// minor has to retain the prototype through the bag, and rewrite
/// the recorded address — the three obligations the owner bag's population
/// shares regardless of kind. Arrays and ordinary objects are the controls:
/// they were covered before.
#[test]
fn test_residual_prototype_owners_of_every_movable_kind_survive_a_copying_minor() {
    let _serialized = crate::array::test_serialize();
    let _feedback = crate::typed_feedback::typed_feedback_test_lock();
    let _latch = ArrayPrototypeLatchRestore::capture();
    // One rooted owner at a time; its prototype is deliberately unrooted.
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_key_table_scanners();
    // RegExp resolves its intrinsic from the lazy realm after the preceding
    // copying minors. The isolation guard removed the normal realm scanner;
    // restore it so that cache is marked and rewritten instead of naming a
    // prototype freed by an earlier iteration. The custom prototypes below
    // remain unrooted and are retained only through their tested owner edge.
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    register_runtime_handle_root_scanner_for_tests();

    type Alloc = Box<dyn Fn() -> usize>;
    let mut owners: Vec<(&str, Alloc)> = vec![
        (
            "array",
            Box::new(|| crate::array::js_array_alloc(4) as usize),
        ),
        (
            "object",
            Box::new(|| crate::object::js_object_alloc(0, 1) as usize),
        ),
        ("lazy array", Box::new(|| nursery_lazy_array(b"[1,2,3]"))),
        ("Map", Box::new(|| crate::map::js_map_alloc(0) as usize)),
        ("Set", Box::new(|| crate::set::js_set_alloc(0) as usize)),
        ("Error", Box::new(|| crate::error::js_error_new() as usize)),
        (
            "Promise",
            Box::new(|| crate::promise::js_promise_new() as usize),
        ),
        (
            "Date",
            Box::new(|| (crate::date_hook_slots::js_date_new().to_bits() & POINTER_MASK) as usize),
        ),
    ];
    #[cfg(feature = "regex-engine")]
    owners.push((
        "RegExp",
        Box::new(|| {
            let pattern = crate::string::js_string_from_bytes(b"a+".as_ptr(), 2);
            let flags = crate::string::js_string_from_bytes(b"g".as_ptr(), 1);
            crate::regex::js_regexp_new(pattern, flags) as usize
        }),
    ));

    for (kind, alloc) in owners {
        let owner = alloc();
        let obj_type = obj_type_at(owner);
        assert!(
            crate::arena::pointer_in_nursery(owner) && crate::gc::gc_type_is_movable(obj_type),
            "{kind}: premise: a nursery owner of a movable kind"
        );
        js_shadow_slot_set(0, ptr_bits(owner));
        let proto = marked_prototype();
        crate::object::js_object_set_prototype_of(
            f64::from_bits(ptr_bits(owner)),
            f64::from_bits(ptr_bits(proto)),
        );
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_eq!(
            crate::object::prototype_chain::object_static_prototype(owner),
            Some(ptr_bits(proto)),
            "{kind}: premise: the prototype is recorded for this owner"
        );

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        assert_ne!(
            owner_after, owner,
            "{kind}: premise: the owner must actually move"
        );

        let recorded = crate::object::prototype_chain::object_static_prototype(owner_after);
        js_shadow_slot_set(0, 0);

        let Some(recorded) = recorded else {
            panic!("{kind}: the owner bag entry did not follow its owner to the new address");
        };
        let proto_after = (recorded & POINTER_MASK) as usize;
        assert_ne!(
            proto_after, proto,
            "{kind}: the recorded prototype still names its pre-collection address — the \
             prototype was either not retained through the entry or not rewritten"
        );
        assert!(
            crate::arena::pointer_in_nursery(proto_after)
                || crate::arena::pointer_in_old_gen(proto_after),
            "{kind}: the recorded prototype must be a live heap address"
        );
        let marker = crate::object::js_object_get_field(
            proto_after as *const crate::object::ObjectHeader,
            0,
        );
        assert_eq!(
            marker.bits(),
            MARKER.to_bits(),
            "{kind}: the recorded address must hold the prototype that was set"
        );
    }
}
