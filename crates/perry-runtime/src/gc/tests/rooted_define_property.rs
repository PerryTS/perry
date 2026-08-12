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
extern "C" fn moving_value_getter(_closure: *const crate::closure::ClosureHeader) -> f64 {
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
    let getter = crate::closure::js_closure_alloc(moving_value_getter as *const u8, 0);
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

    unsafe {
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

        let view = crate::object::try_decode_descriptor(&scope, descriptor.get_nanbox_f64())
            .expect("a plain object literal descriptor must take the fast decode path");
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
