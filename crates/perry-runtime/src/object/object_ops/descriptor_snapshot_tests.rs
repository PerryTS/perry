//! Descriptor snapshots must survive mutation, abrupt completion and forwarding.
use super::*;
use crate::gc::RuntimeHandleScope;

fn key(name: &str) -> f64 {
    f64::from_bits(
        crate::JSValue::string_ptr(crate::string::js_string_from_bytes(
            name.as_ptr(),
            name.len() as u32,
        ))
        .bits(),
    )
}
fn object(scope: &RuntimeHandleScope) -> crate::gc::RuntimeHandle<'_> {
    scope.root_nanbox_f64(crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64))
}
unsafe fn put(obj: f64, name: &str, value: f64) {
    super::super::js_object_set_property_key(obj, key(name), value);
}
unsafe fn get(obj: f64, name: &str) -> f64 {
    super::super::js_object_get_property_key(obj, key(name))
}
unsafe fn data<'scope>(
    scope: &'scope RuntimeHandleScope,
    value: f64,
) -> crate::gc::RuntimeHandle<'scope> {
    let desc = object(scope);
    put(desc.get_nanbox_f64(), "value", value);
    put(
        desc.get_nanbox_f64(),
        "writable",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    desc
}
unsafe fn accessor(obj: f64, name: &str, getter: f64) {
    let obj = extract_obj_ptr(obj);
    let name_key =
        crate::value::js_get_string_pointer_unified(key(name)) as *const crate::StringHeader;
    ensure_key_in_keys_array(obj, name_key);
    set_accessor_descriptor(
        obj as usize,
        name.to_string(),
        AccessorDescriptor {
            get: getter.to_bits(),
            set: 0,
        },
    );
    set_property_attrs(
        obj as usize,
        name.to_string(),
        PropertyAttrs::new(false, true, true),
    );
}
unsafe fn closure(callee: *const crate::closure::JsFunctionInfo, captures: &[f64]) -> f64 {
    let ptr = crate::closure::js_closure_alloc(callee, captures.len() as u32);
    for (index, value) in captures.iter().enumerate() {
        crate::closure::js_closure_set_capture_f64(ptr, index as u32, *value);
    }
    crate::value::js_nanbox_pointer(ptr as i64)
}

#[test]
fn descriptor_snapshot_repeated_errors_release_buffers_and_roots() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let valid = data(&scope, 7.0);
        for late in [false, true] {
            let bag = object(&scope);
            if late {
                for index in 0..32 {
                    put(
                        bag.get_nanbox_f64(),
                        &format!("p{index}"),
                        valid.get_nanbox_f64(),
                    );
                }
            }
            put(bag.get_nanbox_f64(), "invalid", 3.0);
            let depth = RuntimeHandleScope::active_len_for_tests();
            for _ in 0..64 {
                assert!(
                    crate::exception::catch_js_throw(|| js_object_define_properties(
                        target.get_nanbox_f64(),
                        bag.get_nanbox_f64()
                    ))
                    .is_err()
                );
                assert_eq!(RuntimeHandleScope::active_len_for_tests(), depth);
                assert_eq!(
                    super::define_properties::live_collection_bytes_for_tests(),
                    0
                );
                assert_eq!(
                    get(target.get_nanbox_f64(), "p0").to_bits(),
                    crate::value::TAG_UNDEFINED
                );
            }
        }
        let bag = object(&scope);
        put(bag.get_nanbox_f64(), "recovered", valid.get_nanbox_f64());
        js_object_define_properties(target.get_nanbox_f64(), bag.get_nanbox_f64());
        assert_eq!(get(target.get_nanbox_f64(), "recovered"), 7.0);
        assert_eq!(
            super::define_properties::live_collection_bytes_for_tests(),
            0
        );
    }
}

extern "C" fn mutate_earlier(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    unsafe {
        let first = crate::closure::js_closure_get_capture_f64(closure, 0);
        let target = crate::closure::js_closure_get_capture_f64(closure, 1);
        assert_eq!(
            get(target, "a").to_bits(),
            crate::value::TAG_UNDEFINED,
            "collection must precede every write"
        );
        put(first, "value", 100.0);
        put(first, "writable", f64::from_bits(crate::value::TAG_FALSE));
        put(target, "user_effect", 1.0);
        crate::closure::js_closure_get_capture_f64(closure, 2)
    }
}
#[test]
fn descriptor_snapshot_saved_fields_and_user_effects() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let bag = object(&scope);
        let first = data(&scope, 9.0);
        let second = data(&scope, 2.0);
        put(bag.get_nanbox_f64(), "a", first.get_nanbox_f64());
        let getter = scope.root_nanbox_f64(closure(
            crate::fn_info!(mutate_earlier, 0),
            &[
                first.get_nanbox_f64(),
                target.get_nanbox_f64(),
                second.get_nanbox_f64(),
            ],
        ));
        accessor(bag.get_nanbox_f64(), "b", getter.get_nanbox_f64());
        js_object_define_properties(target.get_nanbox_f64(), bag.get_nanbox_f64());
        assert_eq!(get(target.get_nanbox_f64(), "a"), 9.0);
        assert!(
            get_property_attrs(extract_obj_ptr(target.get_nanbox_f64()) as usize, "a")
                .unwrap()
                .writable()
        );
        assert_eq!(get(target.get_nanbox_f64(), "user_effect"), 1.0);
    }
}

thread_local! { static SETTER_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
extern "C" fn setter_field_getter(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    SETTER_READS.with(|reads| reads.set(reads.get() + 1));
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
#[test]
fn descriptor_snapshot_invalid_getter_precedes_setter_get_and_mixed_error() {
    unsafe {
        SETTER_READS.with(|reads| reads.set(0));
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let bag = data(&scope, 1.0);
        put(bag.get_nanbox_f64(), "get", 2.0);
        let getter = scope.root_nanbox_f64(closure(crate::fn_info!(setter_field_getter, 0), &[]));
        accessor(bag.get_nanbox_f64(), "set", getter.get_nanbox_f64());
        assert!(
            crate::exception::catch_js_throw(|| js_object_define_property(
                target.get_nanbox_f64(),
                key("x"),
                bag.get_nanbox_f64()
            ))
            .is_err()
        );
        assert_eq!(SETTER_READS.with(|reads| reads.get()), 0);
    }
}

extern "C" fn mutate_proxy_copy(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _target: f64,
    _key: f64,
    bag: f64,
) -> f64 {
    unsafe {
        put(bag, "value", 1.0);
    }
    f64::from_bits(crate::value::TAG_TRUE)
}
#[test]
fn descriptor_snapshot_proxy_copy_cannot_change_invariant_facts() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let current = data(&scope, 1.0);
        put(
            current.get_nanbox_f64(),
            "writable",
            f64::from_bits(crate::value::TAG_FALSE),
        );
        js_object_define_property(target.get_nanbox_f64(), key("x"), current.get_nanbox_f64());
        let handler = object(&scope);
        let trap = scope.root_nanbox_f64(closure(crate::fn_info!(mutate_proxy_copy, 3), &[]));
        put(
            handler.get_nanbox_f64(),
            "defineProperty",
            trap.get_nanbox_f64(),
        );
        let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            target.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        let change = data(&scope, 2.0);
        assert!(
            crate::exception::catch_js_throw(|| crate::proxy::js_reflect_define_property(
                proxy.get_nanbox_f64(),
                key("x"),
                change.get_nanbox_f64()
            ))
            .is_err()
        );
        assert_eq!(get(change.get_nanbox_f64(), "value"), 2.0);
        assert_eq!(get(target.get_nanbox_f64(), "x"), 1.0);
    }
}

#[test]
fn descriptor_snapshot_array_rejections_are_boolean_through_forwarding() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let index = scope.root_nanbox_f64(key("0"));
        let named = scope.root_nanbox_f64(key("added"));
        let symbol = scope.root_nanbox_f64(crate::symbol::js_symbol_new(key("added")));
        let handler = object(&scope);
        let desc = data(&scope, 1.0);
        for forwarded in [false, true] {
            for saved_key in [&index, &named, &symbol] {
                let array = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                    crate::array::js_array_alloc(0) as i64,
                ));
                super::super::js_object_prevent_extensions(array.get_nanbox_f64());
                let receiver = scope.root_nanbox_f64(if forwarded {
                    crate::proxy::js_proxy_new(array.get_nanbox_f64(), handler.get_nanbox_f64())
                } else {
                    array.get_nanbox_f64()
                });
                let result = crate::exception::catch_js_throw(|| {
                    crate::proxy::js_reflect_define_property(
                        receiver.get_nanbox_f64(),
                        saved_key.get_nanbox_f64(),
                        desc.get_nanbox_f64(),
                    )
                })
                .expect("Reflect array addition rejection must not throw");
                assert_eq!(result.to_bits(), crate::value::TAG_FALSE);
                assert!(
                    crate::exception::catch_js_throw(|| js_object_define_property(
                        receiver.get_nanbox_f64(),
                        saved_key.get_nanbox_f64(),
                        desc.get_nanbox_f64(),
                    ))
                    .is_err(),
                    "Object must throw for the same array rejection"
                );
            }
            let array = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
                crate::array::js_array_alloc(0) as i64,
            ));
            let fixed = object(&scope);
            put(fixed.get_nanbox_f64(), "value", 1.0);
            js_object_define_property(
                array.get_nanbox_f64(),
                symbol.get_nanbox_f64(),
                fixed.get_nanbox_f64(),
            );
            let change = object(&scope);
            put(change.get_nanbox_f64(), "value", 2.0);
            let receiver = scope.root_nanbox_f64(if forwarded {
                crate::proxy::js_proxy_new(array.get_nanbox_f64(), handler.get_nanbox_f64())
            } else {
                array.get_nanbox_f64()
            });
            let result = crate::exception::catch_js_throw(|| {
                crate::proxy::js_reflect_define_property(
                    receiver.get_nanbox_f64(),
                    symbol.get_nanbox_f64(),
                    change.get_nanbox_f64(),
                )
            })
            .expect("Reflect incompatible array symbol must not throw");
            assert_eq!(result.to_bits(), crate::value::TAG_FALSE);
            assert!(
                crate::exception::catch_js_throw(|| js_object_define_property(
                    receiver.get_nanbox_f64(),
                    symbol.get_nanbox_f64(),
                    change.get_nanbox_f64(),
                ))
                .is_err()
            );
        }
    }
}

#[test]
fn descriptor_snapshot_reflect_rejection_and_present_undefined_flag() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let initial = data(&scope, 1.0);
        put(
            initial.get_nanbox_f64(),
            "configurable",
            f64::from_bits(crate::value::TAG_TRUE),
        );
        js_object_define_property(target.get_nanbox_f64(), key("x"), initial.get_nanbox_f64());
        let flags = object(&scope);
        put(
            flags.get_nanbox_f64(),
            "writable",
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert_eq!(
            crate::proxy::js_reflect_define_property(
                target.get_nanbox_f64(),
                key("x"),
                flags.get_nanbox_f64()
            )
            .to_bits(),
            crate::value::TAG_TRUE
        );
        assert!(
            !get_property_attrs(extract_obj_ptr(target.get_nanbox_f64()) as usize, "x")
                .unwrap()
                .writable()
        );
        put(
            flags.get_nanbox_f64(),
            "configurable",
            f64::from_bits(crate::value::TAG_FALSE),
        );
        js_object_define_property(target.get_nanbox_f64(), key("x"), flags.get_nanbox_f64());
        let change = data(&scope, 2.0);
        assert_eq!(
            crate::proxy::js_reflect_define_property(
                target.get_nanbox_f64(),
                key("x"),
                change.get_nanbox_f64()
            )
            .to_bits(),
            crate::value::TAG_FALSE
        );
        assert!(
            crate::exception::catch_js_throw(|| js_object_define_property(
                target.get_nanbox_f64(),
                key("x"),
                change.get_nanbox_f64()
            ))
            .is_err()
        );
    }
}

thread_local! { static FIELD_EVENTS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn field_event(prefix: &str, value: f64) {
    unsafe {
        let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
        let bytes =
            std::slice::from_raw_parts(crate::string::string_data(ptr), (*ptr).byte_len as usize);
        FIELD_EVENTS.with(|events| {
            events
                .borrow_mut()
                .push(format!("{prefix}:{}", std::str::from_utf8(bytes).unwrap()))
        });
    }
}
extern "C" fn field_has(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    target: f64,
    key: f64,
) -> f64 {
    field_event("has", key);
    super::super::js_object_has_property(target, key)
}
extern "C" fn field_get(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    target: f64,
    key: f64,
    _receiver: f64,
) -> f64 {
    field_event("get", key);
    unsafe { super::super::js_object_get_property_key(target, key) }
}
#[test]
fn descriptor_snapshot_generic_has_get_order() {
    unsafe {
        FIELD_EVENTS.with(|events| events.borrow_mut().clear());
        let scope = RuntimeHandleScope::new();
        let fields = data(&scope, 7.0);
        let handler = object(&scope);
        let target = object(&scope);
        put(fields.get_nanbox_f64(), "enumerable", 1.0);
        put(fields.get_nanbox_f64(), "configurable", 0.0);
        let has = scope.root_nanbox_f64(closure(crate::fn_info!(field_has, 2), &[]));
        let get = scope.root_nanbox_f64(closure(crate::fn_info!(field_get, 3), &[]));
        put(handler.get_nanbox_f64(), "has", has.get_nanbox_f64());
        put(handler.get_nanbox_f64(), "get", get.get_nanbox_f64());
        let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            fields.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        js_object_define_property(target.get_nanbox_f64(), key("x"), proxy.get_nanbox_f64());
        assert_eq!(
            FIELD_EVENTS.with(|events| events.borrow().clone()),
            [
                "has:enumerable",
                "get:enumerable",
                "has:configurable",
                "get:configurable",
                "has:value",
                "get:value",
                "has:writable",
                "get:writable",
                "has:get",
                "has:set"
            ]
        );
    }
}

extern "C" fn mutate_key_set(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    unsafe {
        let bag = crate::closure::js_closure_get_capture_f64(closure, 0);
        let descriptor = crate::closure::js_closure_get_capture_f64(closure, 1);
        set_property_attrs(
            extract_obj_ptr(bag) as usize,
            "b".into(),
            PropertyAttrs::new(true, false, true),
        );
        put(bag, "added", descriptor);
        let symbol = crate::closure::js_closure_get_capture_f64(closure, 2);
        crate::symbol::js_object_set_symbol_property(bag, symbol, descriptor);
        descriptor
    }
}
#[test]
fn descriptor_snapshot_keys_precede_callbacks_and_enumerability_is_current() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let bag = object(&scope);
        let descriptor = data(&scope, 1.0);
        let symbol = scope.root_nanbox_f64(crate::symbol::js_symbol_new(key("added_symbol")));
        let getter = scope.root_nanbox_f64(closure(
            crate::fn_info!(mutate_key_set, 0),
            &[
                bag.get_nanbox_f64(),
                descriptor.get_nanbox_f64(),
                symbol.get_nanbox_f64(),
            ],
        ));
        accessor(bag.get_nanbox_f64(), "a", getter.get_nanbox_f64());
        put(bag.get_nanbox_f64(), "b", descriptor.get_nanbox_f64());
        js_object_define_properties(target.get_nanbox_f64(), bag.get_nanbox_f64());
        assert_eq!(get(target.get_nanbox_f64(), "a"), 1.0);
        assert_eq!(
            get(target.get_nanbox_f64(), "b").to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(
            get(target.get_nanbox_f64(), "added").to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(!crate::symbol::has_own_symbol_property(
            target.get_nanbox_f64(),
            symbol.get_nanbox_f64()
        ));
    }
}

thread_local! {static VALUE_READS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
extern "C" fn counted_value(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    VALUE_READS.with(|reads| reads.set(reads.get() + 1));
    1.0
}
#[test]
fn descriptor_snapshot_nested_proxy_forwarding_decodes_once() {
    unsafe {
        VALUE_READS.with(|reads| reads.set(0));
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let handler = object(&scope);
        let bag = object(&scope);
        let getter = scope.root_nanbox_f64(closure(crate::fn_info!(counted_value, 0), &[]));
        accessor(bag.get_nanbox_f64(), "value", getter.get_nanbox_f64());
        let inner = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            target.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        let outer = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            inner.get_nanbox_f64(),
            handler.get_nanbox_f64(),
        ));
        assert_eq!(
            crate::proxy::js_reflect_define_property(
                outer.get_nanbox_f64(),
                key("x"),
                bag.get_nanbox_f64()
            )
            .to_bits(),
            crate::value::TAG_TRUE
        );
        assert_eq!(VALUE_READS.with(|reads| reads.get()), 1);
        assert_eq!(get(target.get_nanbox_f64(), "x"), 1.0);
    }
}

#[test]
fn descriptor_snapshot_internal_own_records_ignore_inherited_fields() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let record = data(&scope, 3.0);
        let prototype = object(&scope);
        put(prototype.get_nanbox_f64(), "get", 2.0);
        js_object_set_prototype_of(record.get_nanbox_f64(), prototype.get_nanbox_f64());
        let view = decode_own_descriptor_result(&scope, &record);
        assert!(view.has(DESC_VALUE));
        assert!(!view.has(DESC_GET));
        assert_eq!(view.read(DESC_VALUE).bits(), 3.0f64.to_bits());
        assert!(
            crate::exception::catch_js_throw(|| decode_property_descriptor(&scope, &record))
                .is_err(),
            "the same inherited field must remain observable on a user descriptor bag"
        );
    }
}

#[test]
fn descriptor_snapshot_exotic_generic_redefinitions_retain_values_and_accessors() {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let array = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::array::js_array_alloc(0) as i64,
        ));
        let typed = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::typedarray::js_typed_array_new_empty(1, 1) as i64,
        ));
        let function = scope.root_nanbox_f64(closure(crate::fn_info!(counted_value, 0), &[]));
        let getter = scope.root_nanbox_f64(closure(crate::fn_info!(counted_value, 0), &[]));
        for receiver in [
            array.get_nanbox_f64(),
            typed.get_nanbox_f64(),
            function.get_nanbox_f64(),
        ] {
            let desc = object(&scope);
            put(desc.get_nanbox_f64(), "get", getter.get_nanbox_f64());
            put(
                desc.get_nanbox_f64(),
                "configurable",
                f64::from_bits(crate::value::TAG_TRUE),
            );
            js_object_define_property(receiver, key("access"), desc.get_nanbox_f64());
            let generic = object(&scope);
            put(
                generic.get_nanbox_f64(),
                "enumerable",
                f64::from_bits(crate::value::TAG_TRUE),
            );
            assert_eq!(
                crate::proxy::js_reflect_define_property(
                    receiver,
                    key("access"),
                    generic.get_nanbox_f64()
                )
                .to_bits(),
                crate::value::TAG_TRUE
            );
            assert_eq!(get(receiver, "access"), 1.0);
            let desc = data(&scope, 7.0);
            put(
                desc.get_nanbox_f64(),
                "configurable",
                f64::from_bits(crate::value::TAG_TRUE),
            );
            js_object_define_property(receiver, key("data"), desc.get_nanbox_f64());
            js_object_define_property(receiver, key("data"), generic.get_nanbox_f64());
            assert_eq!(get(receiver, "data"), 7.0);
        }
    }
}

#[test]
fn descriptor_snapshot_later_application_failure_preserves_earlier_definitions() {
    unsafe {
        VALUE_READS.with(|reads| reads.set(0));
        let scope = RuntimeHandleScope::new();
        let target = object(&scope);
        let fixed = object(&scope);
        put(fixed.get_nanbox_f64(), "value", 1.0);
        js_object_define_property(target.get_nanbox_f64(), key("b"), fixed.get_nanbox_f64());
        let bag = object(&scope);
        let first = data(&scope, 7.0);
        put(bag.get_nanbox_f64(), "a", first.get_nanbox_f64());
        let second = data(&scope, 2.0);
        put(bag.get_nanbox_f64(), "b", second.get_nanbox_f64());
        let last = object(&scope);
        let getter = scope.root_nanbox_f64(closure(crate::fn_info!(counted_value, 0), &[]));
        accessor(last.get_nanbox_f64(), "value", getter.get_nanbox_f64());
        put(bag.get_nanbox_f64(), "c", last.get_nanbox_f64());
        let depth = RuntimeHandleScope::active_len_for_tests();
        assert!(
            crate::exception::catch_js_throw(|| js_object_define_properties(
                target.get_nanbox_f64(),
                bag.get_nanbox_f64()
            ))
            .is_err()
        );
        assert_eq!(
            VALUE_READS.with(|reads| reads.get()),
            1,
            "all descriptors collect before any application rejection"
        );
        assert_eq!(get(target.get_nanbox_f64(), "a"), 7.0);
        assert_eq!(get(target.get_nanbox_f64(), "b"), 1.0);
        assert_eq!(
            get(target.get_nanbox_f64(), "c").to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), depth);
        assert_eq!(
            super::define_properties::live_collection_bytes_for_tests(),
            0
        );
    }
}
