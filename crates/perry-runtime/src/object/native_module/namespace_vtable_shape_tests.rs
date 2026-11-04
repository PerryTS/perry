//! Q8: the module vtable is a fact of the namespace's SHAPE. Namespaces of
//! different modules share one key list (`__module__`) but not one shape, and
//! every transition (add / delete property, preventExtensions, seal, freeze)
//! carries the vtable to the successor shape.

use super::*;
use crate::object::shapes::{object_shape_descriptor, object_shape_stamp};

/// The namespace of `module`, born with the vtable its declaration names.
fn born(vt: &'static NativeModuleVtable, module: &str) -> *mut ObjectHeader {
    let value = crate::object::native_module::create_native_module_namespace(vt, module);
    crate::value::js_nanbox_get_pointer(value) as *mut ObjectHeader
}

fn value_of(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn vt_of(obj: *mut ObjectHeader) -> *const NativeModuleVtable {
    unsafe { object_native_vtable(obj) }.map_or(std::ptr::null(), |vt| vt as *const _)
}

fn stamp(obj: *mut ObjectHeader) -> u64 {
    u64::from(unsafe { object_shape_stamp(obj).0 })
}

#[test]
fn each_module_namespace_has_its_own_shape() {
    let fs = born(&crate::fs::namespace::NM_FS, "fs");
    let path = born(&super::namespaces::path::NM_PATH, "path");
    let opaque = born(&NATIVE_MODULE_VTABLE_IMPL, "perry/gc");
    assert_eq!(vt_of(fs), &crate::fs::namespace::NM_FS as *const _);
    assert_eq!(vt_of(path), &super::namespaces::path::NM_PATH as *const _);
    assert_eq!(vt_of(opaque), &NATIVE_MODULE_VTABLE_IMPL as *const _);
    // Same keys (`__module__`), different vtable: different shapes.
    unsafe {
        let a = object_shape_descriptor(fs).unwrap();
        let b = object_shape_descriptor(path).unwrap();
        assert_eq!(a.logical_key_count, b.logical_key_count);
    }
    assert_ne!(stamp(fs), stamp(path));
    assert_ne!(stamp(fs), stamp(opaque));
    // A second birth of a module reuses its shape.
    let fs_twin = born(&crate::fs::namespace::NM_FS, "fs");
    assert_eq!(stamp(fs_twin), stamp(fs));
}

#[test]
fn a_vtable_without_a_key_table_is_not_empty() {
    // A module with no key table enumerates its key list; it must not read
    // as a declared module with zero exports.
    let opaque = born(&NATIVE_MODULE_VTABLE_IMPL, "perry/gc");
    let vt = unsafe { object_native_vtable(opaque) }.unwrap();
    assert!((vt.keys)(vt).is_none());
    let fs = born(&crate::fs::namespace::NM_FS, "fs");
    let vt = unsafe { object_native_vtable(fs) }.unwrap();
    assert!((vt.keys)(vt).is_some_and(|keys| keys.len() > 50));
}

#[test]
fn adding_and_deleting_a_property_keep_the_vtable() {
    let path = born(&super::namespaces::path::NM_PATH, "path");
    let before = stamp(path);
    let name = "extra_q8";
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    super::super::define_builtin_data_property(
        path,
        key,
        1.0,
        name.to_string(),
        super::super::PropertyAttrs::new(true, true, true),
    );
    let path = born(&super::namespaces::path::NM_PATH, "path");
    let added = stamp(path);
    assert_ne!(added, before, "defineProperty transitions the shape");
    assert_eq!(vt_of(path), &super::namespaces::path::NM_PATH as *const _);
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let deleted =
        js_object_delete_dynamic_value(value_of(path), crate::value::js_nanbox_string(key as i64));
    assert_ne!(deleted, 0);
    let path = born(&super::namespaces::path::NM_PATH, "path");
    assert_ne!(stamp(path), added, "delete transitions the shape");
    assert_eq!(vt_of(path), &super::namespaces::path::NM_PATH as *const _);
}

#[test]
fn integrity_transitions_keep_the_vtable() {
    for (module_vt, module, op) in [
        (
            &super::namespaces::os::NM_OS as &'static NativeModuleVtable,
            "os",
            crate::object::js_object_prevent_extensions as extern "C" fn(f64) -> f64,
        ),
        (
            &super::namespaces::url::NM_URL,
            "url",
            crate::object::js_object_seal,
        ),
        (
            &super::namespaces::querystring::NM_QUERYSTRING,
            "querystring",
            crate::object::js_object_freeze,
        ),
    ] {
        let ns = born(module_vt, module);
        let vt = vt_of(ns);
        assert!(!vt.is_null(), "{module}: born branded");
        let before = stamp(ns);
        op(value_of(ns));
        let ns = born(module_vt, module);
        assert_ne!(
            stamp(ns),
            before,
            "{module}: the integrity level transitions the shape"
        );
        assert_eq!(vt_of(ns), vt, "{module}: the vtable is carried");
    }
    // `module` is sealed at birth: the seal carried the brand too.
    let module = born(&super::namespaces::module::NM_MODULE, "module");
    assert_eq!(
        vt_of(module),
        &super::namespaces::module::NM_MODULE as *const _
    );
}

#[test]
fn the_brand_is_not_a_key() {
    let fs = born(&crate::fs::namespace::NM_FS, "fs");
    let keys = unsafe { vt_own_keys_array(fs) }.unwrap();
    let count = crate::array::js_array_length(keys) as usize;
    assert_eq!(count, crate::fs::namespace::NM_FS.module_keys.len());
    unsafe {
        let shape = object_shape_descriptor(fs).unwrap();
        assert_eq!(shape.logical_key_count, 1, "only the `__module__` slot");
    }
}

#[test]
fn a_default_callable_births_one_base_namespace_with_the_base_vtable() {
    use super::namespaces::os::{NM_OS, NM_OS_DEFAULT};
    // A program without a dynamic require: no module rows, so no name finds a
    // vtable.
    assert!(native_module_vtable_by_name("os").is_none());
    assert!(std::ptr::eq(
        NM_OS_DEFAULT.base_namespace.unwrap()(),
        &NM_OS
    ));
    let cached = || NATIVE_MODULE_NAMESPACES.with(|c| c.borrow().get("os").copied());
    assert_eq!(cached(), None);
    // `import os from "os"; os.hostname()`: the callable names the default's
    // vtable and births the base namespace it dispatches through.
    let hostname =
        crate::object::bound_native_callable_export_value(&NM_OS_DEFAULT, "os.default", "hostname");
    assert_ne!(hostname.to_bits(), crate::value::TAG_UNDEFINED);
    let first = cached().expect("the callable births the base namespace");
    // `import * as m from "os"`: the same object, keyed with the base vtable.
    let ns = born(&NM_OS, "os");
    assert_eq!(value_of(ns).to_bits(), first);
    assert_eq!(vt_of(ns), &NM_OS as *const _);
}
