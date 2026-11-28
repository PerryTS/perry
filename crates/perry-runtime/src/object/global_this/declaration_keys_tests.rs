//! The guard that keeps wpfacts' realm model and the runtime in step: the
//! own string keys of each live builtin prototype equal the keys the realm
//! declaration (`realm-v1.json`, the table wpfacts reads) gives it. A member
//! the runtime installs but the declaration omits makes a declared-chain walk
//! claim a lookup resolves further up than it does (`Error.prototype.valueOf`
//! was an own copy at run time and absent from the declaration).
use super::*;

fn own_string_keys(proto: f64) -> std::collections::BTreeSet<String> {
    let names = crate::object::js_object_get_own_property_names(proto);
    let names = crate::value::js_nanbox_get_pointer(names) as *const crate::array::ArrayHeader;
    (0..crate::array::js_array_length(names))
        .map(|i| {
            let s = crate::builtins::js_string_coerce(crate::array::js_array_get_f64(names, i));
            unsafe {
                let bytes = std::slice::from_raw_parts(
                    crate::string::string_data(s),
                    (*s).byte_len as usize,
                );
                String::from_utf8_lossy(bytes).into_owned()
            }
        })
        .collect()
}

#[test]
fn live_builtin_prototype_own_keys_equal_the_realm_declaration() {
    let _gc = crate::gc::GcSuppressScope::new();
    let realm: serde_json::Value =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/realm-v1.json")))
            .expect("realm-v1.json");
    let mut checked = 0usize;
    let mut mismatches = Vec::new();
    for decl in realm["declarations"].as_array().expect("declarations") {
        let name = decl["name"].as_str().expect("name");
        let Some(ctor) = name.strip_suffix(".prototype") else {
            continue;
        };
        if decl["kind"] != "prototype" || ctor.contains(['%', '.']) {
            continue;
        }
        // External families' members are their provider's (the stdlib's
        // streams install, not linked here): perry-stdlib's
        // `stream_prototypes_hold_the_declared_keys` checks them.
        if crate::object::BuiltinConstructorIdentity::for_name(ctor)
            .is_some_and(|identity| identity.declaration().external)
        {
            continue;
        }
        let proto = builtin_prototype_value(ctor);
        if proto.to_bits() == crate::value::TAG_UNDEFINED {
            continue;
        }
        let declared: std::collections::BTreeSet<String> = decl["own_key_order"]
            .as_array()
            .expect("own_key_order")
            .iter()
            .map(|k| k.as_str().expect("key").to_string())
            .collect();
        let live = own_string_keys(proto);
        checked += 1;
        if live != declared {
            mismatches.push(format!(
                "{name}: runtime-only {:?}, declared-only {:?}",
                live.difference(&declared).collect::<Vec<_>>(),
                declared.difference(&live).collect::<Vec<_>>()
            ));
        }
    }
    assert!(checked >= 60, "only {checked} builtin prototypes resolved");
    assert!(
        mismatches.is_empty(),
        "{} of {checked} builtin prototypes differ from the realm declaration:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

fn js_key(key: &str) -> f64 {
    let s = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::value::js_nanbox_string(s as i64)
}

fn field(obj: f64, key: &str) -> f64 {
    let s = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(obj) as *const crate::object::ObjectHeader,
        s,
    )
}

/// Own string keys of a live function, and whether `getOwnPropertyDescriptor`
/// finds its `prototype`: the two must agree, and equal the declaration.
fn function_keys(f: f64) -> (std::collections::BTreeSet<String>, bool) {
    let keys = own_string_keys(f);
    let desc = crate::object::js_object_get_own_property_descriptor(f, js_key("prototype"));
    (keys, desc.to_bits() != crate::value::TAG_UNDEFINED)
}

/// A built-in getter or setter is not a constructor (ECMA-262 10.3): every
/// install path births it without an own `prototype`, as the realm declares
/// (Map/Set `size`, the ArrayBuffer, TypedArray, DataView and RegExp
/// accessors, Intl.Locale's). The Proxy constructor has no `prototype`
/// (ECMA-262 28.2).
#[test]
fn live_builtin_accessor_functions_and_proxy_own_keys_equal_the_realm_declaration() {
    let _gc = crate::gc::GcSuppressScope::new();
    let realm: serde_json::Value =
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/realm-v1.json")))
            .expect("realm-v1.json");
    let mut checked = Vec::new();
    let mut mismatches = Vec::new();
    let mut proxy = None;
    for decl in realm["declarations"].as_array().expect("declarations") {
        let name = decl["name"].as_str().expect("name");
        // Only functions carry `own_key_order` (exotic, namespace, constant,
        // alias and birth-site declarations have none).
        let declared = || -> std::collections::BTreeSet<String> {
            decl["own_key_order"]
                .as_array()
                .expect("own_key_order")
                .iter()
                .map(|k| k.as_str().expect("key").to_string())
                .collect()
        };
        if name == "Proxy" {
            proxy = Some(declared());
            continue;
        }
        let (accessor, slot) = match decl["kind"].as_str() {
            Some("getter") => (name.strip_suffix(".<get>"), "get"),
            Some("setter") => (name.strip_suffix(".<set>"), "set"),
            _ => continue,
        };
        let declared = declared();
        let Some((ctor, key)) = accessor.and_then(|a| a.split_once(".prototype.")) else {
            continue;
        };
        if ctor.contains(['%', '.', ':', '[']) || key.contains(['.', '[', '"']) {
            continue;
        }
        let proto = builtin_prototype_value(ctor);
        if proto.to_bits() == crate::value::TAG_UNDEFINED {
            continue;
        }
        let desc = crate::object::js_object_get_own_property_descriptor(proto, js_key(key));
        if desc.to_bits() == crate::value::TAG_UNDEFINED {
            continue;
        }
        let f = field(desc, slot);
        if !crate::value::JSValue::from_bits(f.to_bits()).is_pointer() {
            continue;
        }
        let (live, described) = function_keys(f);
        if live != declared || described != live.contains("prototype") {
            mismatches.push(format!(
                "{name}: live {live:?} (descriptor finds prototype: {described}), declared {declared:?}"
            ));
        }
        checked.push(name.to_string());
    }
    for must in [
        "Map.prototype.size.<get>",
        "Set.prototype.size.<get>",
        "ArrayBuffer.prototype.byteLength.<get>",
        "DataView.prototype.buffer.<get>",
        "RegExp.prototype.flags.<get>",
    ] {
        assert!(checked.iter().any(|c| c == must), "{must} was not reached");
    }
    let declared = proxy.expect("the realm declares Proxy");
    assert!(!declared.contains("prototype"), "Proxy declares an own prototype");
    let ctor = field(crate::object::js_get_global_this(), "Proxy");
    let (live, described) = function_keys(ctor);
    if live != declared || described {
        mismatches.push(format!("Proxy: live {live:?} (descriptor: {described}), declared {declared:?}"));
    }
    assert!(
        mismatches.is_empty(),
        "{} built-in functions differ from the realm declaration:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
