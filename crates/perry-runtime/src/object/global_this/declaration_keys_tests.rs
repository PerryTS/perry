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
                    (s as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
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
