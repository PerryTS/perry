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
