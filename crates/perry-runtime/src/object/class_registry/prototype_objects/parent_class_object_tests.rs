//! #10890: an instance of a declared class whose heritage is a per-evaluation
//! class OBJECT (`class InitError extends makeClass(...) {}`) reads that
//! evaluation's prototype, never the parent constructor's own properties.
//!
//! Built through the entry points compiled code calls: a class object marked
//! with `js_object_mark_class`, its prototype materialized by a `.prototype`
//! read, and the heritage edge recorded by `js_register_class_parent_dynamic`.

use super::super::{
    class_prototype_object, instance_class_prototype_object, is_class_object_ptr,
    js_object_mark_class, js_register_class_id, js_register_class_name,
    js_register_class_parent_dynamic, resolve_proto_chain_field, test_alloc_synthetic_class_id,
};
use crate::object::{
    class_prototype_object_root_store, js_object_alloc, js_object_get_field_by_name,
    js_object_set_field_by_name, ObjectHeader,
};
use crate::JSValue;

unsafe fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn string_value(text: &str) -> f64 {
    f64::from_bits(crate::js_nanbox_string(key(text) as i64).to_bits())
}

unsafe fn read(obj: *const ObjectHeader, name: &str) -> JSValue {
    js_object_get_field_by_name(obj, key(name))
}

unsafe fn read_string(obj: *const ObjectHeader, name: &str) -> Option<String> {
    let value = read(obj, name);
    if !value.is_string() {
        return None;
    }
    let s = value.as_string_ptr();
    let bytes = std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// A heap class object for `template`, named `out` and carrying one static.
unsafe fn class_object(template: u32) -> *mut ObjectHeader {
    js_register_class_id(template);
    js_register_class_name(template, b"out".as_ptr(), 3);
    let class = js_object_alloc(template, 1);
    js_object_mark_class(class as i64);
    js_object_set_field_by_name(class, key("identifier"), string_value("static"));
    class
}

#[test]
fn an_instance_reads_the_parent_evaluations_prototype_not_the_parent_constructor() {
    let _lock = crate::gc::global_side_table_test_lock();
    const TEMPLATE: u32 = 0x1089_0001;
    const CHILD: u32 = 0x1089_0002;
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let class = scope.root_raw_mut_ptr(class_object(TEMPLATE));
        let class_value = class.with_const_ptr::<ObjectHeader, _>(|class| {
            f64::from_bits(crate::value::js_nanbox_pointer(class as i64).to_bits())
        });
        // `Object.assign(out.prototype, { name: identifier })`.
        let proto = class.with_const_ptr::<ObjectHeader, _>(|class| read(class, "prototype"));
        assert!(
            proto.is_pointer(),
            "reading `.prototype` must materialize it"
        );
        let proto = scope.root_raw_mut_ptr(proto.as_pointer::<ObjectHeader>() as *mut ObjectHeader);
        proto.with_mut_ptr::<ObjectHeader, _>(|proto| {
            js_object_set_field_by_name(proto, key("name"), string_value("ProviderInitError"))
        });

        js_register_class_id(CHILD);
        js_register_class_parent_dynamic(CHILD, class_value);
        class.with_const_ptr::<ObjectHeader, _>(|class| {
            assert_eq!(
                class_prototype_object(CHILD) as usize,
                class as usize,
                "fixture must record the parent CLASS OBJECT for the declared child, \
                 or every verdict below is vacuous"
            );
            assert!(is_class_object_ptr(class as *const u8));
        });
        assert!(
            instance_class_prototype_object(CHILD).is_null(),
            "a parent class object is not on an instance's prototype chain"
        );

        let instance = scope.root_raw_mut_ptr(js_object_alloc(CHILD, 0));
        let name = instance.with_const_ptr::<ObjectHeader, _>(|inst| read_string(inst, "name"));
        assert_eq!(
            name.as_deref(),
            Some("ProviderInitError"),
            "`name` comes from the parent's prototype, not the parent's own `name` (`out`)"
        );
        let leaked = instance.with_const_ptr::<ObjectHeader, _>(|inst| read(inst, "identifier"));
        assert!(
            leaked.is_undefined(),
            "a parent static must not surface on an instance"
        );

        // The constructor side still inherits the parent's statics.
        let inherited = resolve_proto_chain_field(CHILD, key("identifier"));
        assert!(
            inherited.is_some_and(|value| value.is_string()),
            "the subclass constructor still reads the parent class object's static"
        );
    }
}

/// `Object.create(C)` where `C` is a class object: the synthetic id's entry
/// IS the instance's prototype, so its own properties stay readable.
#[test]
fn a_synthetic_prototype_that_is_a_class_object_stays_on_the_instance_chain() {
    let _lock = crate::gc::global_side_table_test_lock();
    const TEMPLATE: u32 = 0x1089_0003;
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let class = scope.root_raw_mut_ptr(class_object(TEMPLATE));
        let synthetic = test_alloc_synthetic_class_id();
        assert_ne!(synthetic, 0, "the synthetic id range is exhausted");
        class.with_mut_ptr::<ObjectHeader, _>(|class| {
            class_prototype_object_root_store(synthetic, class);
            assert_eq!(
                instance_class_prototype_object(synthetic) as usize,
                class as usize
            );
        });
        let instance = scope.root_raw_mut_ptr(js_object_alloc(synthetic, 0));
        let value =
            instance.with_const_ptr::<ObjectHeader, _>(|inst| read_string(inst, "identifier"));
        assert_eq!(value.as_deref(), Some("static"));
    }
}
