//! #7268 — `JSON.stringify`'s homogeneous-array shape template held three raw
//! heap pointers across a collection point, and the collection point is user
//! code.
//!
//! `json/stringify_shape_template.rs::try_emit_shape_element` derived the
//! element header and its inline `fields_ptr` **once**, then looped over the
//! fields calling `stringify_value_depth` for the pointer-valued ones. That
//! call can run a user `toJSON`, allocate, and take an evacuating minor with
//! it. Three holders were exposed:
//!
//!   1. `elem_ptr` / `fields_ptr` — a bare Rust local, so every later
//!      `fields_ptr.add(f)` read a forwarded or swept address;
//!   2. `ShapeTemplate::keys_arr` — the raw `ArrayHeader*` used as the shape
//!      identity AND dereferenced by `set_to_json_key_for_template_field` to
//!      read the property-name strings back out;
//!   3. `SHAPE_CACHE` itself, whose doc comment asserted the invariant that
//!      made (2) safe — *"within one top-level stringify call no GC runs over
//!      the user object graph"* — which `toJSON` falsifies.
//!
//! ## The collection point needs no user JS to reach
//!
//! A `Date` field is enough: `stringify_value_depth`'s `is_date_cell_addr`
//! branch calls `js_date_to_json`, which **allocates a `StringHeader`**. That
//! makes the hazard reachable from a pure-runtime unit test — no closure
//! plumbing, no `.ts` witness, no knob. The issue's suggested shape (a later
//! element carrying an allocating `toJSON`) is the same window reached the hard
//! way.
//!
//! ## Element order is load-bearing
//!
//! The probe object is `{ when: Date, answer: 42, also: Date }`. `when` is the
//! allocation; `answer` is read from `fields_ptr` AFTER it (hazard 1); `also`
//! runs `set_to_json_key_for_template_field`, which dereferences `keys_arr`,
//! after it (hazard 2). A two-field object would exercise neither.

use super::*;

/// `{ when: <date>, answer: 42, also: <date> }` — see the module header for why
/// this exact shape.
///
/// Built under suppressed triggers: `js_object_set_field_by_name` allocates the
/// keys array and each `alloc_date_cell` allocates, and a collection landing
/// there would move `obj` — a bare local here — out from under the setup.
unsafe fn probe_object(_trigger_guard: &GcTriggerThresholdTestGuard) -> *mut crate::ObjectHeader {
    let obj = crate::object::js_object_alloc(0, 3);
    let when = crate::date::alloc_date_cell(0.0);
    let also = crate::date::alloc_date_cell(86_400_000.0);
    let when_key = crate::string::js_string_from_bytes(b"when".as_ptr(), 4);
    crate::object::js_object_set_field_by_name(obj, when_key, when);
    let answer_key = crate::string::js_string_from_bytes(b"answer".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(obj, answer_key, 42.0);
    let also_key = crate::string::js_string_from_bytes(b"also".as_ptr(), 4);
    crate::object::js_object_set_field_by_name(obj, also_key, also);
    obj
}

fn string_contents(ptr: *const crate::StringHeader) -> String {
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// The end-to-end witness. An array of two same-shape objects goes through the
/// template emit path; the `Date` in field 0 of each element allocates, the
/// armed trigger turns that allocation into an evacuating minor, and fields 1
/// and 2 must still read correctly afterwards.
///
/// Note which template this exercises: `stringify_array_depth` builds its
/// template into a plain `Option<ShapeTemplate>` **Rust local**, not into
/// `SHAPE_CACHE`. No root scanner can ever see that one — which is why the fix
/// roots `keys_arr` inside `try_emit_shape_element` rather than relying on the
/// cache scanner alone. The cache scanner is covered by the sibling test below.
#[test]
fn shape_template_element_survives_the_date_field_allocation() {
    // This test needs nursery pressure to reach the DIRECT allocation-point
    // minor: it asserts both that a bounded assist ran and — as its liveness
    // witness — that the minor EVACUATED. The default moving-loop pacing routes
    // that pressure into the safepoint deferral instead, and a Rust unit test
    // has no loop back-edge poll to drain it, so no collection happens at all.
    // Legacy pacing is not the answer either: it hands the work to the budgeted
    // stepper, which is deliberately non-moving, and the evacuation witness then
    // correctly refuses to certify an empty run. `force_alloc_point_minor_pacing`
    // is the combination this test was written against and the only one in which
    // both halves hold. The moving default's rooting coverage for these helpers
    // is the gap suite's `test_gap_gc_*_rooting.ts` cases plus the rate-1 schedule +
    // from-space-protect runs, not this vehicle.
    let _alloc_point_pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    crate::json::test_clear_parse_roots();

    let (arr_bits, expected) = unsafe {
        let a = probe_object(&trigger_guard);
        let b = probe_object(&trigger_guard);
        let arr = crate::array::js_array_alloc(2);
        let arr = crate::array::js_array_push_f64(arr, f64::from_bits(ptr_bits(a as usize)));
        let arr = crate::array::js_array_push_f64(arr, f64::from_bits(ptr_bits(b as usize)));

        // Establish the expected bytes with NO collection in flight, so the
        // assertion below compares against this build's own rendering rather
        // than a hard-coded ISO string that a date-formatting change would
        // silently invalidate.
        let baseline = crate::json::js_json_stringify(f64::from_bits(ptr_bits(arr as usize)), 0);
        (ptr_bits(arr as usize), string_contents(baseline))
    };
    assert!(
        expected.contains("\"answer\":42"),
        "the probe never reached the template emit path (got {expected}) — \
         without `answer` in the output this test proves nothing"
    );

    // Liveness witness: a rooted sentinel must relocate, or the cycle below
    // moved nothing and a green result is meaningless.
    let sentinel_scope = RuntimeHandleScope::new();
    let sentinel = sentinel_scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let sentinel_before = sentinel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize;

    // Keep the array reachable across the collection the way generated code
    // would, so the SUBJECT of the test is the template path's own rooting and
    // not the caller's.
    let arr_scope = RuntimeHandleScope::new();
    let arr_root = arr_scope.root_nanbox_u64(arr_bits);

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();

    let out = unsafe { crate::json::js_json_stringify(arr_root.get_nanbox_f64(), 0) };
    let out_scope = RuntimeHandleScope::new();
    let out_root = out_scope.root_string_ptr(out);
    drain_scheduled_minor_gc(before, "Date field stringification");
    let actual = string_contents(out_root.get_raw_const_ptr::<crate::StringHeader>());

    assert_ne!(
        sentinel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize,
        sentinel_before,
        "the minor did not evacuate — nothing here was exercised"
    );
    assert_eq!(
        actual, expected,
        "the shape-template emit path read a field through an address it \
         computed BEFORE the Date allocation collected"
    );
}

/// The cache half. `SHAPE_CACHE` is a registered GC root now
/// (`json::scan_parse_roots_mut`), and both halves have to hold: MARKING keeps
/// the keys array off the sweep list, and REWRITING keeps the identity key —
/// and the property-name strings read out of it — pointing at the object rather
/// than at whatever gets recycled into the from-space address.
///
/// A scanner a test calls directly is a no-op in production until `gc_init`
/// names it, so registration is asserted too.
#[test]
fn stringify_shape_cache_keys_array_is_marked_and_rewritten() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::json::test_clear_parse_roots();

    clear_marks();
    clear_mark_seeds();

    let keys_arr = unsafe {
        let obj = crate::object::js_object_alloc(0, 1);
        let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
        crate::object::js_object_set_field_by_name(obj, key, 1.0);
        let keys = crate::object::object_keys_array(obj);
        assert!(!keys.is_null(), "probe object must have a keys array");
        assert!(
            crate::arena::pointer_in_nursery(keys as usize),
            "the keys array must be movable or the rewrite half tests nothing"
        );
        crate::json::test_seed_stringify_shape_cache(keys);
        keys
    };

    // MARK. The cache is a root, so a live cache entry must survive a cycle
    // that has no other reference to the keys array.
    let valid_ptrs = build_valid_pointer_set();
    crate::json::scan_parse_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid_ptrs));
    assert_marked_user_ptr(keys_arr as usize, "shape-cache keys array");

    // REWRITE. Marking alone is not enough: an un-rewritten cache hands out a
    // pre-move address forever, which is the whole failure.
    let moved = unsafe {
        let to = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
        set_forwarding_address(header_from_user_ptr(keys_arr as *const u8), to);
        to as usize
    };
    crate::json::scan_parse_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid_ptrs));
    assert_eq!(
        crate::json::test_stringify_shape_cache_keys(),
        vec![moved],
        "scan_parse_roots_mut must rewrite SHAPE_CACHE's keys_array through \
         the forwarding address"
    );

    crate::json::test_clear_parse_roots();
}

/// A scanner that is not in the registry is documentation. `scan_parse_roots_mut`
/// rides on the registered `json_parse_mutable_root_scanner`; this asserts the
/// shape cache is covered by a REGISTERED scanner rather than by one only this
/// file calls.
#[test]
fn stringify_shape_cache_scanner_is_registered() {
    crate::gc::gc_init();
    let registered = crate::gc::roots::MUTABLE_ROOT_SCANNERS.with(|scanners| {
        scanners.borrow().iter().any(|entry| {
            entry.scanner as usize
                == crate::gc::roots::json_parse_mutable_root_scanner as MutableRootScanner as usize
        })
    });
    assert!(
        registered,
        "json_parse_mutable_root_scanner (which owns SHAPE_CACHE since #7268) \
         is not in the mutable root scanner registry — the cache would be \
         unrooted in production no matter what the scanner body says"
    );
}
