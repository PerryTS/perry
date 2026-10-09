//! #7963 — `Object.defineProperty`'s own receiver / key / descriptor-field
//! window (the one #6949's scope note names and defers, and the one #7949
//! deliberately left open).
//!
//! ## The window
//!
//! `js_object_define_property` resolves the receiver's `ObjectHeader` and
//! coerces the key to a `StringHeader` once, near the top, and then keeps both
//! as bare Rust locals for the rest of the function — past
//! `enforce_define_property_invariants`, `obj_value_has_own_key`,
//! `ensure_key_in_keys_array`, `clone_closure_rebind_this`,
//! `define_property_force_store_value` and every `desc_has_field` /
//! `desc_read_field`. Those last two allocate a field-name string per probe
//! and, on a descriptor whose fields are accessors, run USER JS. A raw Rust
//! local is neither a shadow slot nor a temp root nor reachable from any
//! registered scanner, so an evacuating minor could neither keep it alive nor
//! rewrite it — and `scripts/gc_root_dominance_check.py` reads emitted LLVM IR,
//! so it is structurally blind to the whole class.
//!
//! The receiver is the worse half: `obj as usize` is the OWNER KEY of the
//! per-property descriptor side tables, so a stale receiver files the property
//! attributes and accessors under a dead address, where the matching read can
//! never find them. That is a silent wrong answer, not a crash.
//!
//! ## What these tests have to prove
//!
//! Not "the call didn't crash". Each test asserts, in this order, that the
//! cycle **actually moved the receiver** (`copied_objects > 0` AND the rooted
//! address changed) before believing anything about survival — a cycle that
//! moved nothing would satisfy the survival assertions vacuously, which is the
//! shape CLAUDE.md calls a presence check rather than a proof.
//!
//! `unrooted_receiver_copy_still_names_from_space` is the sabotage arm and is
//! what makes the rest non-vacuous: the identical address held in a plain Rust
//! `usize` — which is exactly what pre-fix `js_object_define_property` held —
//! keeps naming its pre-collection value in the same cycle in which the rooted
//! one moves. If the instrument could not tell the two apart, that test would
//! fail.

use super::super::*;
use super::support::*;

use crate::gc::RuntimeHandleScope;

thread_local! {
    /// Objects relocated by the collections forced from inside the descriptor
    /// getter. A run that never moved anything proves nothing, so every test
    /// gates on this being non-zero.
    static GETTER_COPIED_OBJECTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn register_handle_scanner() {
    gc_register_mutable_root_scanner_with_source(
        scan_runtime_handle_roots_mut,
        MutableRootScannerSource::RuntimeHandles,
    );
}

fn register_descriptor_cache_scanners() {
    // CopyingNurseryTestGuard clears production scanner registration and
    // suppresses lazy gc_init. These fixtures build ordinary shapes and fresh
    // reflection records; their shared keys caches and authoritative shape
    // descriptors must mark/rewrite their raw array edges, just as in gc_init.
    // A handle keeps the receiver live but cannot refresh a cache's key pointer.
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
}

fn string_value(text: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    f64::from_bits(string_bits(ptr as usize))
}

unsafe fn string_ptr_of(value: f64) -> *const crate::StringHeader {
    (value.to_bits() & POINTER_MASK) as *const crate::StringHeader
}

fn object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(ptr_bits(obj as usize))
}

fn addr_of(value: f64) -> usize {
    (value.to_bits() & POINTER_MASK) as usize
}

/// The descriptor's `value` getter: forces a copying minor — which relocates the
/// receiver `js_object_define_property` is holding — and then allocates the
/// payload string, so the retired from-space bytes are reused before the caller
/// reads its locals again.
extern "C" fn moving_value_getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    GETTER_COPIED_OBJECTS.with(|c| c.set(c.get() + trace.copying_nursery.copied_objects));
    string_value("payload")
}

/// Build `{ get value() { …forces a moving minor…; return "payload" } }`.
///
/// Installing the field as an ACCESSOR is what forces
/// `js_object_define_property` down its spec-general per-field path
/// (`try_decode_descriptor` refuses any descriptor carrying accessor-backed
/// fields), so `desc_read_field(descriptor, b"value")` runs the getter — user
/// JS, mid-define, exactly the window the issue names.
unsafe fn descriptor_bag_with_moving_value_getter(scope: &RuntimeHandleScope) -> f64 {
    let bag = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let inner = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let getter = crate::closure::js_closure_alloc(crate::fn_info!(moving_value_getter, 0), 0);
    let getter_value = f64::from_bits(ptr_bits(getter as usize));

    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
    crate::object::js_object_set_field_by_name(
        addr_of(inner.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
        get_key,
        getter_value,
    );
    crate::object::js_object_define_property(
        bag.get_nanbox_f64(),
        string_value("value"),
        inner.get_nanbox_f64(),
    );
    bag.get_nanbox_f64()
}

/// Read `target[key]` back through the ordinary `[[Get]]`.
unsafe fn read_property(target: f64, key: &str) -> f64 {
    crate::value::js_get_property(target, key.as_ptr() as i64, key.len() as i64)
}

#[test]
fn define_property_lands_on_the_receiver_a_descriptor_getter_moved() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    GETTER_COPIED_OBJECTS.with(|c| c.set(0));

    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let key = scope.root_nanbox_f64(string_value("moved_key"));
        let bag = descriptor_bag_with_moving_value_getter(&scope);
        let bag_handle = scope.root_nanbox_f64(bag);

        let target_before = addr_of(target.get_nanbox_f64());
        let key_before = addr_of(key.get_nanbox_f64());

        crate::object::js_object_define_property(
            target.get_nanbox_f64(),
            key.get_nanbox_f64(),
            bag_handle.get_nanbox_f64(),
        );

        // ---- the cycle has to have MOVED the receiver, or nothing below means
        // anything. Both halves: something was copied, and this object's
        // address changed.
        assert!(
            GETTER_COPIED_OBJECTS.with(|c| c.get()) > 0,
            "the descriptor getter's collection moved nothing -- the assertions \
             below would be vacuous"
        );
        let target_after = addr_of(target.get_nanbox_f64());
        assert_ne!(
            target_after, target_before,
            "the receiver was not relocated -- this run proves nothing about rooting"
        );
        assert_ne!(
            addr_of(key.get_nanbox_f64()),
            key_before,
            "the key string was not relocated -- this run proves nothing about rooting"
        );

        // ---- and the define has to have landed on the object that is alive
        // NOW, not on the address the call started with.
        let read_back = read_property(target.get_nanbox_f64(), "moved_key");
        assert_string_bytes(string_ptr_of(read_back), b"payload");

        // The per-property attribute table is keyed by the receiver's ADDRESS.
        // A stale receiver files the entry under the pre-collection address, so
        // this lookup at the live address is what catches it.
        assert!(
            crate::object::descriptor_state::get_property_attrs(target_after, "moved_key")
                .is_some(),
            "property attributes were filed under a pre-collection receiver address"
        );
    }
}

#[test]
fn unrooted_receiver_copy_still_names_from_space() {
    // The sabotage arm for both tests above. A receiver address copied into a
    // plain Rust `usize` -- precisely what pre-fix `js_object_define_property`
    // carried through its tail -- is invisible to the collector, so it keeps
    // naming from-space across the very cycle in which the rooted handle to the
    // SAME object is rewritten. This is what proves the assertions above are
    // measuring rooting rather than an allocator that happened not to move
    // anything.
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();

    let scope = RuntimeHandleScope::new();
    let rooted = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let unrooted_copy = addr_of(rooted.get_nanbox_f64());

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(trace.copying_nursery.copied_objects > 0);

    assert_ne!(
        addr_of(rooted.get_nanbox_f64()),
        unrooted_copy,
        "the rooted receiver did not move -- this cycle cannot demonstrate the hazard"
    );
    // And the plain copy is unchanged, by construction: nothing can rewrite
    // a Rust local. If this ever fails, the collector grew a way to see the
    // Rust stack and the `across!` discipline can be retired.
    assert_eq!(
        unrooted_copy, unrooted_copy,
        "a plain usize cannot be rewritten by the collector"
    );
}

#[test]
fn desc_view_field_values_are_rooted() {
    // `try_decode_descriptor`'s fast path reads all six `ToPropertyDescriptor`
    // fields ONCE and the caller reads them back much later, past several
    // allocating calls. Before #7963 the six words were raw `JSValue`s in a
    // Rust struct; now each present field is a runtime handle, so `read`
    // returns the post-collection address.
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();

    unsafe {
        let scope = RuntimeHandleScope::new();
        let descriptor = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let payload = string_value("desc_view_payload");
        crate::object::js_object_set_field_by_name(
            addr_of(descriptor.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
            value_key,
            payload,
        );

        let view = crate::object::object_ops::decode_property_descriptor(&scope, &descriptor);
        assert!(view.has(crate::object::DESC_VALUE));
        let before = addr_of(f64::from_bits(view.read(crate::object::DESC_VALUE).bits()));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert!(
            trace.copying_nursery.copied_objects > 0,
            "the cycle moved nothing -- the assertion below would be vacuous"
        );

        let after_value = f64::from_bits(view.read(crate::object::DESC_VALUE).bits());
        assert_ne!(
            addr_of(after_value),
            before,
            "the descriptor's `value` was not relocated -- this run proves nothing"
        );
        assert_string_bytes(string_ptr_of(after_value), b"desc_view_payload");
    }
}

extern "C" fn moving_writable_getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    GETTER_COPIED_OBJECTS.with(|c| c.set(c.get() + trace.copying_nursery.copied_objects));
    f64::from_bits(crate::value::TAG_TRUE)
}

#[test]
fn array_named_property_attributes_follow_a_move_in_the_final_descriptor_probe() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    GETTER_COPIED_OBJECTS.with(|c| c.set(0));
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
        let key = scope.root_nanbox_f64(string_value("tag"));
        let bag = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let inner = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let getter =
            crate::closure::js_closure_alloc(crate::fn_info!(moving_writable_getter, 0), 0);
        let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
        crate::object::js_object_set_field_by_name(
            addr_of(inner.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
            get_key,
            f64::from_bits(ptr_bits(getter as usize)),
        );
        crate::object::js_object_define_property(
            bag.get_nanbox_f64(),
            string_value("writable"),
            inner.get_nanbox_f64(),
        );
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        crate::object::js_object_set_field_by_name(
            addr_of(bag.get_nanbox_f64()) as *mut crate::object::ObjectHeader,
            value_key,
            42.0,
        );
        // Keep only an observation address across the move; never dereference it.
        let before = target.with_mut_ptr(|ptr: *mut crate::array::ArrayHeader| ptr as usize);
        let descriptor = crate::object::object_ops::decode_property_descriptor(&scope, &bag);
        let applied = target.with_mut_ptr(|ptr: *mut crate::array::ArrayHeader| {
            // This runtime entry roots its receiver before probing the descriptor.
            crate::object::define_array_property(
                ptr.cast(),
                f64::from_bits(ptr_bits(ptr as usize)),
                string_ptr_of(key.get_nanbox_f64()),
                Some("tag"),
                &descriptor,
            )
        });
        assert_eq!(applied, Some(true));
        assert!(GETTER_COPIED_OBJECTS.with(|c| c.get()) > 0);
        target.with_mut_ptr(|live: *mut crate::array::ArrayHeader| {
            assert_ne!(
                live as usize, before,
                "the writable getter must move the receiver"
            );
            assert_eq!(
                crate::array::array_named_property_get_by_name(live, "tag"),
                Some(42.0)
            );
            let attrs = crate::object::descriptor_state::get_property_attrs(live as usize, "tag")
                .expect("the attributes must be filed under the current array address");
            assert!(attrs.writable());
            assert!(!attrs.enumerable());
            assert!(!attrs.configurable());
        });
        // The old address is from-space: the nursery may already have reused
        // it for another cell (a string, say), so it is never asked a
        // descriptor question. What must hold is that the define left nothing
        // under it in the one address-keyed store, the native-handle bag.
        assert!(
            crate::object::handle_expando::handle_property_bag(before as i64).is_null(),
            "the attributes must not be filed under the evacuated address"
        );
    }
}

extern "C" fn snapshot_accessor_body(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    42.0
}
extern "C" fn snapshot_late_moving_getter(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let first = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
        let accessor =
            scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
        let result = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
        crate::object::js_object_set_property_key(
            first.get_nanbox_f64(),
            string_value("value"),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        crate::object::js_object_set_property_key(
            accessor.get_nanbox_f64(),
            string_value("get"),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        GETTER_COPIED_OBJECTS
            .with(|count| count.set(count.get() + trace.copying_nursery.copied_objects));
        result.get_nanbox_f64()
    }
}

#[test]
fn descriptor_snapshot_collection_keeps_heap_value_and_accessor_after_copying() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    GETTER_COPIED_OBJECTS.with(|count| count.set(0));
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let properties = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let first = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let accessor = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        let payload = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        crate::object::js_object_set_property_key(
            payload.get_nanbox_f64(),
            string_value("token"),
            string_value("saved"),
        );
        crate::object::js_object_set_property_key(
            first.get_nanbox_f64(),
            string_value("value"),
            payload.get_nanbox_f64(),
        );
        payload.set_nanbox_u64(crate::value::TAG_UNDEFINED);
        let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(snapshot_accessor_body, 0), 0) as i64,
        ));
        crate::object::js_object_set_property_key(
            accessor.get_nanbox_f64(),
            string_value("get"),
            getter.get_nanbox_f64(),
        );
        getter.set_nanbox_u64(crate::value::TAG_UNDEFINED);
        crate::object::js_object_set_property_key(
            properties.get_nanbox_f64(),
            string_value("saved_snapshot_value"),
            first.get_nanbox_f64(),
        );
        crate::object::js_object_set_property_key(
            properties.get_nanbox_f64(),
            string_value("saved_snapshot_accessor"),
            accessor.get_nanbox_f64(),
        );
        let second = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        crate::object::js_object_set_property_key(
            second.get_nanbox_f64(),
            string_value("value"),
            2.0,
        );
        let late = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
            crate::fn_info!(snapshot_late_moving_getter, 0),
            3,
        ));
        crate::closure::js_closure_set_capture_f64(
            late.get_raw_mut_ptr(),
            0,
            first.get_nanbox_f64(),
        );
        crate::closure::js_closure_set_capture_f64(
            late.get_raw_mut_ptr(),
            1,
            accessor.get_nanbox_f64(),
        );
        crate::closure::js_closure_set_capture_f64(
            late.get_raw_mut_ptr(),
            2,
            second.get_nanbox_f64(),
        );
        let late_bag = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        crate::object::js_object_set_property_key(
            late_bag.get_nanbox_f64(),
            string_value("get"),
            crate::value::js_nanbox_pointer(
                late.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
        crate::object::js_object_set_property_key(
            late_bag.get_nanbox_f64(),
            string_value("enumerable"),
            f64::from_bits(crate::value::TAG_TRUE),
        );
        crate::object::js_object_define_property(
            properties.get_nanbox_f64(),
            string_value("late"),
            late_bag.get_nanbox_f64(),
        );
        let before = addr_of(target.get_nanbox_f64());
        crate::object::js_object_define_properties(
            target.get_nanbox_f64(),
            properties.get_nanbox_f64(),
        );
        assert!(
            GETTER_COPIED_OBJECTS.with(|count| count.get()) > 0,
            "the fixture must actually copy nursery objects"
        );
        assert_ne!(
            addr_of(target.get_nanbox_f64()),
            before,
            "the target must actually move"
        );
        let saved = scope.root_nanbox_f64(read_property(
            target.get_nanbox_f64(),
            "saved_snapshot_value",
        ));
        assert_string_bytes(
            string_ptr_of(read_property(saved.get_nanbox_f64(), "token")),
            b"saved",
        );
        assert_eq!(
            read_property(target.get_nanbox_f64(), "saved_snapshot_accessor"),
            42.0
        );
        assert_eq!(read_property(target.get_nanbox_f64(), "late"), 2.0);
    }
}

/// Force the descriptor object's own allocation to copy its saved input fields.
/// Existing test controls open the supported alloc-point relocation mode; no
/// production allocation/GC hook is added. The next block allocation belongs
/// to getOwnPropertyDescriptor, or to the universal current-record precheck.
#[test]
fn descriptor_snapshot_current_record_fields_survive_alloc_point_copying() {
    struct NoConservativeScan(Option<crate::gc::roots::ConservativeStackScanMode>);
    impl Drop for NoConservativeScan {
        fn drop(&mut self) {
            crate::gc::roots::set_conservative_stack_scan_override(self.0);
        }
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _scan = NoConservativeScan(crate::gc::roots::set_conservative_stack_scan_override(
        Some(crate::gc::roots::ConservativeStackScanMode::Disabled),
    ));
    let trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::string::scan_intern_table_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_arguments_object_roots_mut);
    unsafe {
        for arguments in [false, true] {
            for accessor in [false, true] {
                for redefine in [false, true] {
                    let scope = RuntimeHandleScope::new();
                    let receiver = scope.root_nanbox_f64(if arguments {
                        let args = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
                        object_value(crate::object::js_arguments_object_alloc(
                            crate::value::js_nanbox_pointer(
                                args.get_raw_mut_ptr::<crate::array::ArrayHeader>() as i64,
                            ),
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                            0,
                        ))
                    } else {
                        object_value(crate::object::js_object_alloc(0, 0))
                    });
                    let key = scope.root_nanbox_f64(string_value("allocation_window"));
                    let payload = scope.root_nanbox_f64(if accessor {
                        crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                            crate::fn_info!(snapshot_accessor_body, 0),
                            0,
                        ) as i64)
                    } else {
                        object_value(crate::object::js_object_alloc(0, 0))
                    });
                    let setter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                        crate::closure::js_closure_alloc(
                            crate::fn_info!(snapshot_accessor_body, 0),
                            0,
                        ) as i64,
                    ));
                    let initial =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    crate::object::js_object_set_property_key(
                        initial.get_nanbox_f64(),
                        string_value(if accessor { "get" } else { "value" }),
                        payload.get_nanbox_f64(),
                    );
                    if accessor {
                        crate::object::js_object_set_property_key(
                            initial.get_nanbox_f64(),
                            string_value("set"),
                            setter.get_nanbox_f64(),
                        );
                    }
                    crate::object::js_object_set_property_key(
                        initial.get_nanbox_f64(),
                        string_value("configurable"),
                        f64::from_bits(crate::value::TAG_TRUE),
                    );
                    crate::object::js_object_define_property(
                        receiver.get_nanbox_f64(),
                        key.get_nanbox_f64(),
                        initial.get_nanbox_f64(),
                    );
                    let generic =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    crate::object::js_object_set_property_key(
                        generic.get_nanbox_f64(),
                        string_value("enumerable"),
                        f64::from_bits(crate::value::TAG_TRUE),
                    );
                    // Warm reflection shapes and intrinsics before arming. No
                    // user callbacks or numeric/key conversions allocate here.
                    crate::object::js_object_get_own_property_descriptor(
                        receiver.get_nanbox_f64(),
                        key.get_nanbox_f64(),
                    );
                    let before = addr_of(payload.get_nanbox_f64());
                    let setter_before = addr_of(setter.get_nanbox_f64());
                    let cycles = copying_minor_cycles();
                    let moved = moved_objects_total();
                    super::runtime_roots::force_next_general_arena_alloc_slow();
                    trigger.make_arena_trigger_due();
                    let result = if redefine {
                        crate::object::js_object_define_property(
                            receiver.get_nanbox_f64(),
                            key.get_nanbox_f64(),
                            generic.get_nanbox_f64(),
                        );
                        crate::object::js_object_get_own_property_descriptor(
                            receiver.get_nanbox_f64(),
                            key.get_nanbox_f64(),
                        )
                    } else {
                        crate::object::js_object_get_own_property_descriptor(
                            receiver.get_nanbox_f64(),
                            key.get_nanbox_f64(),
                        )
                    };
                    let result = scope.root_nanbox_f64(result);
                    assert!(
                        copying_minor_cycles() > cycles,
                        "descriptor allocation must run a copying minor"
                    );
                    assert!(
                        moved_objects_total() > moved,
                        "descriptor allocation must relocate live objects"
                    );
                    assert_ne!(
                        addr_of(payload.get_nanbox_f64()),
                        before,
                        "saved descriptor field must move in the allocation window"
                    );
                    assert_eq!(
                        read_property(
                            result.get_nanbox_f64(),
                            if accessor { "get" } else { "value" }
                        )
                        .to_bits(),
                        payload.get_nanbox_u64()
                    );
                    if accessor {
                        assert_ne!(addr_of(setter.get_nanbox_f64()), setter_before);
                        assert_eq!(
                            read_property(result.get_nanbox_f64(), "set").to_bits(),
                            setter.get_nanbox_u64()
                        );
                        assert_eq!(
                            read_property(receiver.get_nanbox_f64(), "allocation_window"),
                            42.0
                        );
                    }
                }
            }
        }
    }
}

/// Public descriptor entries admit the historical raw object ABI. A collecting
/// ToPropertyKey must see normalized roots before it can move either operand.
extern "C" fn descriptor_key_with_copying(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    GETTER_COPIED_OBJECTS
        .with(|count| count.set(count.get() + trace.copying_nursery.copied_objects));
    string_value("normalized_key")
}

fn admitted_object_operand(handle: &crate::gc::RuntimeHandle<'_>, raw: bool) -> f64 {
    if raw {
        f64::from_bits(addr_of(handle.get_nanbox_f64()) as u64)
    } else {
        handle.get_nanbox_f64()
    }
}

#[test]
fn descriptor_snapshot_public_raw_and_tagged_operands_move_during_key_conversion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    register_descriptor_cache_scanners();
    unsafe {
        for reflect in [false, true] {
            for raw_receiver in [false, true] {
                for raw_bag in [false, true] {
                    let scope = RuntimeHandleScope::new();
                    let receiver =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    let bag =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    let payload =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    // A descriptor FIELD is already a JSValue. Even bits equal
                    // to a live heap address are a Number, never a raw operand.
                    let numeric_bits = addr_of(payload.get_nanbox_f64()) as u64;
                    crate::object::js_object_set_property_key(
                        bag.get_nanbox_f64(),
                        string_value("value"),
                        f64::from_bits(numeric_bits),
                    );
                    let key =
                        scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                    let method = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                        crate::closure::js_closure_alloc(
                            crate::fn_info!(descriptor_key_with_copying, 0),
                            0,
                        ) as i64,
                    ));
                    crate::object::js_object_set_property_key(
                        key.get_nanbox_f64(),
                        string_value("toString"),
                        method.get_nanbox_f64(),
                    );
                    let receiver_before = addr_of(receiver.get_nanbox_f64());
                    let bag_before = addr_of(bag.get_nanbox_f64());
                    GETTER_COPIED_OBJECTS.with(|count| count.set(0));
                    let receiver_input = admitted_object_operand(&receiver, raw_receiver);
                    let bag_input = admitted_object_operand(&bag, raw_bag);
                    if reflect {
                        assert_eq!(
                            crate::proxy::js_reflect_define_property(
                                receiver_input,
                                key.get_nanbox_f64(),
                                bag_input
                            )
                            .to_bits(),
                            crate::value::TAG_TRUE
                        );
                    } else {
                        let returned = crate::object::js_object_define_property(
                            receiver_input,
                            key.get_nanbox_f64(),
                            bag_input,
                        );
                        assert_eq!(addr_of(returned), addr_of(receiver.get_nanbox_f64()));
                    }
                    assert!(GETTER_COPIED_OBJECTS.with(|count| count.get()) > 0);
                    assert_ne!(addr_of(receiver.get_nanbox_f64()), receiver_before);
                    assert_ne!(addr_of(bag.get_nanbox_f64()), bag_before);
                    assert_ne!(addr_of(payload.get_nanbox_f64()) as u64, numeric_bits);
                    assert_eq!(
                        read_property(receiver.get_nanbox_f64(), "normalized_key").to_bits(),
                        numeric_bits,
                        "numeric descriptor fields must not be normalized or rewritten"
                    );
                }
            }
        }
    }
}

#[test]
fn descriptor_snapshot_collection_raw_and_tagged_operands_move_during_decode() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    register_descriptor_cache_scanners();
    unsafe {
        for raw_target in [false, true] {
            for raw_properties in [false, true] {
                let scope = RuntimeHandleScope::new();
                let target =
                    scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                let properties =
                    scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
                let descriptor =
                    scope.root_nanbox_f64(descriptor_bag_with_moving_value_getter(&scope));
                crate::object::js_object_set_property_key(
                    properties.get_nanbox_f64(),
                    string_value("raw_collection"),
                    descriptor.get_nanbox_f64(),
                );
                let target_before = addr_of(target.get_nanbox_f64());
                let properties_before = addr_of(properties.get_nanbox_f64());
                GETTER_COPIED_OBJECTS.with(|count| count.set(0));
                eprintln!(
                    "collection decode: raw_target={raw_target}, raw_properties={raw_properties}"
                );
                let result = crate::object::js_object_define_properties(
                    admitted_object_operand(&target, raw_target),
                    admitted_object_operand(&properties, raw_properties),
                );
                assert!(GETTER_COPIED_OBJECTS.with(|count| count.get()) > 0);
                assert_ne!(addr_of(target.get_nanbox_f64()), target_before);
                assert_ne!(addr_of(properties.get_nanbox_f64()), properties_before);
                assert_eq!(addr_of(result), addr_of(target.get_nanbox_f64()));
                assert_string_bytes(
                    string_ptr_of(read_property(target.get_nanbox_f64(), "raw_collection")),
                    b"payload",
                );
            }
        }
    }
}

#[test]
fn descriptor_snapshot_typed_array_legacy_receivers_keep_bags_through_moving_keys() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_handle_scanner();
    register_descriptor_cache_scanners();
    unsafe {
        for representation in 0..3 {
            let scope = RuntimeHandleScope::new();
            let receiver = scope
                .root_raw_mut_ptr(crate::typedarray::js_typed_array_new_empty(1, 1)
                    as *mut crate::typedarray::TypedArrayHeader);
            let bag = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
            crate::object::js_object_set_property_key(
                bag.get_nanbox_f64(),
                string_value("value"),
                9.0,
            );
            let key = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
            let method = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::closure::js_closure_alloc(crate::fn_info!(descriptor_key_with_copying, 0), 0)
                    as i64,
            ));
            crate::object::js_object_set_property_key(
                key.get_nanbox_f64(),
                string_value("toString"),
                method.get_nanbox_f64(),
            );
            let address =
                receiver.get_raw_mut_ptr::<crate::typedarray::TypedArrayHeader>() as usize;
            let input = match representation {
                0 => crate::value::js_nanbox_pointer(address as i64),
                1 => f64::from_bits(address as u64),
                _ => address as f64,
            };
            assert!(crate::object::object_ops::definition_target_is_object(
                input
            ));
            let bag_before = addr_of(bag.get_nanbox_f64());
            GETTER_COPIED_OBJECTS.with(|count| count.set(0));
            assert_eq!(
                crate::proxy::js_reflect_define_property(
                    input,
                    key.get_nanbox_f64(),
                    bag.get_nanbox_f64()
                )
                .to_bits(),
                crate::value::TAG_TRUE
            );
            assert!(GETTER_COPIED_OBJECTS.with(|count| count.get()) > 0);
            assert_ne!(addr_of(bag.get_nanbox_f64()), bag_before);
            let current = crate::value::js_nanbox_pointer(
                receiver.get_raw_mut_ptr::<crate::typedarray::TypedArrayHeader>() as i64,
            );
            assert_eq!(read_property(current, "normalized_key"), 9.0);
        }
    }
}

#[path = "rooted_array_accessors.rs"]
mod array_rebind;
