//! Object-literal `super[key]` roots its lexical home and original receiver
//! across a key coercion that can run user code and move the nursery.

use super::support::*;
use crate::gc::{gc_register_mutable_root_scanner, GcTriggerKind};
use std::sync::atomic::{AtomicUsize, Ordering};

per_test_global! {
    static SUPER_GET_COPIED_OBJECTS: AtomicUsize = AtomicUsize::new(0);
}

extern "C" fn moving_key_to_string(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _hint: f64,
) -> f64 {
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    SUPER_GET_COPIED_OBJECTS.fetch_add(trace.copying_nursery.copied_objects, Ordering::Relaxed);
    f64::from_bits(crate::value::JSValue::short_string_unchecked(b"value").bits())
}

extern "C" fn getter_reads_receiver_tag(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let receiver = crate::value::JSValue::from_bits(this.as_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>();
    let tag = crate::string::canonical_key(b"tag");
    f64::from_bits(crate::object::js_object_get_field_by_name(receiver, tag).bits())
}

fn object_value() -> f64 {
    crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64)
}

fn address(value: f64) -> usize {
    (value.to_bits() & crate::value::POINTER_MASK) as usize
}

#[test]
fn object_super_get_roots_home_and_receiver_across_moving_key_coercion() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    // The isolation guard resets the production scanner registry. Reinstall
    // object-model roots because the fixture depends on shape descriptors,
    // transition metadata, canonical keys, and symbol properties surviving
    // the forced move.
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    gc_register_mutable_root_scanner(crate::object::canonical_keys::scan_canonical_keys_roots_mut);
    gc_register_mutable_root_scanner(crate::symbol::scan_symbol_side_table_roots_mut);
    SUPER_GET_COPIED_OBJECTS.store(0, Ordering::Relaxed);

    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let home = scope.root_nanbox_f64(object_value());
        let base = scope.root_nanbox_f64(object_value());
        let receiver = scope.root_nanbox_f64(object_value());
        let key_object = scope.root_nanbox_f64(object_value());
        let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(getter_reads_receiver_tag, 0), 0)
                as i64,
        ));
        let key_converter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(
                crate::fn_info!(moving_key_to_string, 1; with_declared(1)),
                0,
            ) as i64,
        ));

        let key_name = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let tag_name = crate::string::canonical_key(b"tag");
        crate::object::js_object_define_accessor(
            base.get_nanbox_f64(),
            crate::value::js_nanbox_string(key_name as i64),
            getter.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        crate::object::js_object_set_field_by_name(
            crate::value::JSValue::from_bits(receiver.get_nanbox_f64().to_bits())
                .as_pointer::<crate::object::ObjectHeader>()
                as *mut crate::object::ObjectHeader,
            tag_name,
            crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                b"moved receiver".as_ptr(),
                14,
            ) as i64),
        );
        crate::object::js_object_set_prototype_of(home.get_nanbox_f64(), base.get_nanbox_f64());
        let to_primitive = crate::symbol::well_known_symbol("toPrimitive");
        crate::symbol::js_object_set_symbol_property(
            key_object.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(to_primitive as i64),
            key_converter.get_nanbox_f64(),
        );

        let home_before = address(home.get_nanbox_f64());
        let receiver_before = address(receiver.get_nanbox_f64());
        let result = crate::object::js_object_super_get(
            home.get_nanbox_f64(),
            key_object.get_nanbox_f64(),
            receiver.get_nanbox_f64(),
        );

        assert!(
            SUPER_GET_COPIED_OBJECTS.load(Ordering::Relaxed) > 0,
            "the key coercion's collection moved nothing; this rooting check would be vacuous"
        );
        assert_ne!(
            address(home.get_nanbox_f64()),
            home_before,
            "the lexical home object did not move during key coercion"
        );
        assert_ne!(
            address(receiver.get_nanbox_f64()),
            receiver_before,
            "the original receiver did not move during key coercion"
        );
        let result = crate::value::JSValue::from_bits(result.to_bits());
        let string = result.as_string_ptr();
        assert_eq!(
            crate::string::header_str_checked(string),
            Some("moved receiver"),
            "the inherited getter must observe the relocated original receiver"
        );
    }
}
