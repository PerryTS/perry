//! #11542: a native-module namespace object (`require("process")`,
//! `require("path")`, ...) is an ordinary object. Its own properties are its
//! module's exports; a read or `in` that misses them continues at its
//! `[[Prototype]]` — the one `Object.getPrototypeOf(ns)` reports — instead of
//! answering `undefined` / `false` from the export table alone.

use super::*;

const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

fn key_str(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn key_value(name: &str) -> f64 {
    crate::value::js_nanbox_string(key_str(name) as i64)
}

fn namespace(module: &str) -> f64 {
    js_create_native_module_namespace(module.as_ptr(), module.len())
}

fn get(receiver: f64, name: &str) -> f64 {
    let obj = crate::value::js_nanbox_get_pointer(receiver) as *const ObjectHeader;
    let value = js_object_get_field_by_name(obj, key_str(name));
    f64::from_bits(value.bits())
}

fn object_ctor() -> f64 {
    js_get_global_this_builtin_value(b"Object".as_ptr(), 6)
}

#[test]
fn namespace_own_miss_reads_its_prototype() {
    for module in ["process", "path", "os"] {
        let ns = namespace(module);
        let proto = js_object_get_prototype_of(ns);
        assert!(
            JSValue::from_bits(proto.to_bits()).is_pointer(),
            "{module}: the namespace must report an object [[Prototype]]"
        );
        assert_eq!(
            get(ns, "constructor").to_bits(),
            object_ctor().to_bits(),
            "{module}: `ns.constructor` is inherited from Object.prototype"
        );
        assert_eq!(
            get(ns, "hasOwnProperty").to_bits(),
            get(proto, "hasOwnProperty").to_bits(),
            "{module}: `ns.hasOwnProperty` is the prototype's"
        );
        // A name neither the namespace nor its prototype has stays absent.
        assert_eq!(
            get(ns, "noSuchExport11542").to_bits(),
            crate::value::TAG_UNDEFINED,
            "{module}: an absent name must still read undefined"
        );
    }
}

#[test]
fn namespace_own_export_still_wins() {
    let ns = namespace("path");
    let join = JSValue::from_bits(get(ns, "join").to_bits());
    assert!(join.is_pointer(), "`path.join` is an own export");
    assert_ne!(join.bits(), object_ctor().to_bits());
}

#[test]
fn namespace_has_property_consults_its_prototype() {
    let ns = namespace("process");
    assert_eq!(
        js_object_has_property(ns, key_value("constructor")).to_bits(),
        TAG_TRUE,
        "`\"constructor\" in ns` is true through Object.prototype"
    );
    assert_eq!(
        js_object_has_own(ns, key_value("constructor")).to_bits(),
        TAG_FALSE,
        "...but it is not an own property"
    );
    assert_eq!(
        js_object_has_property(ns, key_value("noSuchExport11542")).to_bits(),
        TAG_FALSE
    );
}

/// The issue's program: an object created with a namespace as its prototype
/// reads `constructor` THROUGH the namespace, and the result is an object
/// that `Object.getPrototypeOf` / `Object.create` accept.
#[test]
fn read_through_object_create_of_namespace() {
    let ns = namespace("process");
    let obj = js_object_create(ns);
    assert_eq!(
        js_object_get_prototype_of(obj).to_bits(),
        ns.to_bits(),
        "Object.create(ns) must have ns as its [[Prototype]]"
    );
    let ctor = get(obj, "constructor");
    assert_eq!(
        ctor.to_bits(),
        object_ctor().to_bits(),
        "`Object.create(ns).constructor` resolves through ns to Object"
    );
    let proto = js_object_get_prototype_of(ctor);
    assert!(
        JSValue::from_bits(proto.to_bits()).is_pointer(),
        "the constructor's [[Prototype]] is an object"
    );
    let obj2 = js_object_create(proto);
    assert!(JSValue::from_bits(obj2.to_bits()).is_pointer());
}

// nskeys: verify the public creator, including the CommonJS namespace path,
// publishes the brand before any read or write could repair its shape.
#[test]
fn namespace_creation_publishes_brand_and_full_exports() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _gc = crate::gc::GcSuppressScope::new();
    for module in ["zlib", "fs", "path", "path.default", "events", "crypto"] {
        let ns = namespace(module);
        let obj = crate::value::js_nanbox_get_pointer(ns) as *const ObjectHeader;
        unsafe {
            assert_eq!(
                shapes::object_shape_descriptor(obj).unwrap().object_kind,
                shapes::ShapeObjectKind::NativeNamespace,
                "{module}: namespace must be branded at birth"
            );
            let keys = js_object_keys(obj);
            assert!(crate::array::js_array_length(keys) > 1, "{module}: exports");
        }
    }
}

/// #9202: recording a `[[Prototype]]` on a namespace must not hide that
/// namespace's OWN module exports.
///
/// `cluster.default` is the only namespace wired with a prototype
/// (`cluster::cluster_default_value` points it at `EventEmitter.prototype`), and
/// `shape_inherited::get` admitted every `ShapeObjectKind::NativeNamespace`
/// receiver because it shares the ordinary LAYOUT. On an admitted receiver it
/// answered the read with a recorded-prototype chain walk and returned
/// `undefined` on a chain miss — pre-empting the namespace vtable's
/// `vt_get_own_field`, which is where the module table lives. So
/// `process.getBuiltinModule("cluster").isPrimary` read `undefined` while every
/// prototype-less namespace resolved, including `getBuiltinModule("url")` and
/// `getBuiltinModule("stream")` in the same fixture.
#[test]
fn namespace_with_a_recorded_prototype_still_reads_its_exports() {
    let ns = namespace("path");
    let own_before = get(ns, "join");
    assert!(
        JSValue::from_bits(own_before.to_bits()).is_pointer(),
        "fixture must start proven: `path.join` is an own export"
    );

    let addr = crate::value::js_nanbox_get_pointer(ns) as usize;
    let proto = crate::object::js_object_alloc(0, 0);
    crate::object::prototype_chain::object_set_static_prototype(
        addr,
        crate::value::js_nanbox_pointer(proto as i64).to_bits(),
    );

    let own_after = get(ns, "join");
    assert!(
        JSValue::from_bits(own_after.to_bits()).is_pointer(),
        "a recorded [[Prototype]] must not shadow the module's own exports"
    );
    assert_eq!(
        get(ns, "noSuchExport9202").to_bits(),
        crate::value::TAG_UNDEFINED,
        "...while an absent name still reads undefined"
    );
}

/// #13963: a call of an inherited `Object.prototype` predicate on a namespace
/// runs that predicate on the namespace. The module's dispatch bucket has no
/// arm for it and answered `undefined`, breaking safer-buffer's
/// `buffer.hasOwnProperty(key)` copy loop.
#[test]
fn namespace_predicate_call_answers_for_the_namespace() {
    let ns = namespace("path");
    let call = |method: &str, key: &str| {
        let args = [key_value(key)];
        unsafe {
            js_native_call_method(
                ns,
                method.as_ptr() as *const i8,
                method.len(),
                args.as_ptr(),
                args.len(),
            )
        }
        .to_bits()
    };
    assert_eq!(call("hasOwnProperty", "join"), TAG_TRUE);
    assert_eq!(call("hasOwnProperty", "noSuchExport13963"), TAG_FALSE);
    assert_eq!(call("propertyIsEnumerable", "join"), TAG_TRUE);
}

/// #13963: the name-keyed member read a default import lowers to
/// (`buffer.hasOwnProperty`) resolves an inherited `Object.prototype` method
/// instead of answering `undefined` for a name the module does not export.
#[test]
fn default_import_member_read_reaches_object_prototype() {
    let read = |prop: &str| {
        let module = key_value("path");
        js_native_module_esm_export_value(module, key_value(prop))
    };
    let ns = namespace("path");
    let proto = js_object_get_prototype_of(ns);
    assert_eq!(
        read("hasOwnProperty").to_bits(),
        get(proto, "hasOwnProperty").to_bits()
    );
    assert_eq!(read("constructor").to_bits(), object_ctor().to_bits());
    assert_eq!(
        read("noSuchExport13963").to_bits(),
        crate::value::TAG_UNDEFINED,
        "a name neither exported nor inherited stays undefined"
    );
}

// DELNS: `delete ns.m` of a namespace export removes the export.

fn keys_contain(obj: *const ObjectHeader, name: &str) -> bool {
    let keys = js_object_keys(obj);
    (0..crate::array::js_array_length(keys)).any(|i| {
        let v = crate::array::js_array_get_f64(keys, i);
        let s = crate::value::js_get_string_pointer_unified(v) as *const crate::StringHeader;
        unsafe {
            std::slice::from_raw_parts(crate::string::string_data(s), (*s).byte_len as usize)
                == name.as_bytes()
        }
    })
}

/// Node: a CommonJS export is a configurable data property. `delete` removes
/// it (true), every own-key probe stops seeing it, a read continues at the
/// prototype, and a later store re-creates it.
#[test]
fn deleting_a_namespace_export_removes_it_until_a_store() {
    let ns = namespace("path");
    let obj = crate::value::js_nanbox_get_pointer(ns) as *mut ObjectHeader;
    let original = get(ns, "relative");
    assert!(
        JSValue::from_bits(original.to_bits()).is_pointer(),
        "fixture: an own export"
    );
    assert!(keys_contain(obj, "relative"));
    assert_eq!(
        js_object_delete_field_value(ns, key_str("relative")),
        1,
        "delete answers true"
    );
    let obj = crate::value::js_nanbox_get_pointer(ns) as *mut ObjectHeader;
    assert_eq!(get(ns, "relative").to_bits(), crate::value::TAG_UNDEFINED);
    assert_eq!(
        js_object_has_own(ns, key_value("relative")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_object_has_property(ns, key_value("relative")).to_bits(),
        TAG_FALSE
    );
    assert!(
        !keys_contain(obj, "relative"),
        "Object.keys drops the deleted export"
    );
    assert!(keys_contain(obj, "join"), "...and only it");
    // A deleted name that Object.prototype has reads through to it.
    assert_eq!(
        crate::proxy::js_reflect_delete(ns, key_value("join")).to_bits(),
        TAG_TRUE,
        "Reflect.deleteProperty answers true"
    );
    assert_eq!(get(ns, "join").to_bits(), crate::value::TAG_UNDEFINED);
    let name = key_value("relative");
    crate::proxy::js_put_value_set(ns, name, original, ns, 0);
    assert_eq!(
        get(ns, "relative").to_bits(),
        original.to_bits(),
        "a store re-creates it"
    );
    assert_eq!(
        js_object_has_own(ns, key_value("relative")).to_bits(),
        TAG_TRUE
    );
    let join = key_value("join");
    let restored = get(namespace("path.default"), "join");
    crate::proxy::js_put_value_set(ns, join, restored, ns, 0);
}

/// A non-configurable namespace member (`fs.constants`, as its descriptor
/// reports) survives `delete`, which answers false.
#[test]
fn deleting_a_non_configurable_namespace_member_keeps_it() {
    let ns = namespace("fs");
    assert_eq!(
        js_object_delete_dynamic_value(ns, key_value("constants")),
        0
    );
    assert_eq!(
        js_object_has_own(ns, key_value("constants")).to_bits(),
        TAG_TRUE
    );
    assert!(JSValue::from_bits(get(ns, "constants").to_bits()).is_pointer());
}

/// `delete console.log` through a value: the member is gone (a method call of
/// it throws), and `console` is withdrawn so guarded sites stop binding the
/// intrinsic.
#[test]
fn deleting_a_console_member_removes_it() {
    let g = crate::object::js_get_global_this();
    let g = crate::value::js_nanbox_get_pointer(g) as *mut ObjectHeader;
    let console = js_object_get_field_by_name(g, key_str("console"));
    let console = f64::from_bits(console.bits());
    let obj = crate::value::js_nanbox_get_pointer(console) as *mut ObjectHeader;
    assert!(JSValue::from_bits(get(console, "warn").to_bits()).is_pointer());
    assert_eq!(
        js_object_delete_dynamic_value(console, key_value("warn")),
        1
    );
    assert_eq!(get(console, "warn").to_bits(), crate::value::TAG_UNDEFINED);
    assert_eq!(
        js_object_has_own(console, key_value("warn")).to_bits(),
        TAG_FALSE
    );
    assert!(!keys_contain(obj, "warn"));
    assert!(keys_contain(obj, "log"));
    let family = crate::codegen_abi::builtin_namespace_family("console").unwrap();
    assert!(!super::builtin_namespace::pristine(
        family - super::builtin_namespace::FIRST
    ));
}
