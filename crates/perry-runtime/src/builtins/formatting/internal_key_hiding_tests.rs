//! `util.inspect` must not print perry's hidden runtime-internal own keys.
//!
//! These keys physically live in an object's `keys_array` but are not JS
//! properties: `Object.keys`, `for…in`, `getOwnPropertyNames`,
//! `JSON.stringify`, `hasOwnProperty` and spread all filter them through
//! `is_internal_runtime_key`. Inspection was the one own-key consumer that did
//! not, so any object carrying one rendered it in its body where Node prints
//! nothing — most visibly the #10624 constructing-class pin
//! (`__perry_ctor_class_object`) that every instance of a per-evaluation class
//! object carries, and the #6438 parent edge (`__perry_parent_class`) on the
//! class object itself.
//!
//! The key spellings are written out literally rather than imported: the
//! constants live inside the private `object::class_registry` /
//! `object::parent_static` trees. Each case first asserts the spelling it uses
//! really is in the allowlist, so renaming a constant without updating this
//! file fails the precondition instead of silently testing nothing.

use super::format_jsvalue;

/// Allocate a plain object with `keys` set to distinct numeric values, in
/// order, and return its inspected rendering.
fn inspect_object_with_keys(keys: &[&str]) -> String {
    let obj = crate::object::js_object_alloc(0, keys.len() as u32);
    assert!(!obj.is_null(), "fixture object must allocate");
    for (index, name) in keys.iter().enumerate() {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, (index + 1) as f64);
    }
    format_jsvalue(crate::value::js_nanbox_pointer(obj as i64), 0)
}

fn assert_hidden_from_inspect(key: &str) {
    assert!(
        crate::object::is_internal_runtime_key(key),
        "fixture must start proven: '{key}' is not in the internal-key allowlist, \
         so this case would pass vacuously"
    );
    let rendered = inspect_object_with_keys(&["alpha", key]);
    assert!(
        rendered.contains("alpha"),
        "an ordinary own key must survive the filter, got {rendered}"
    );
    assert!(
        !rendered.contains(key),
        "'{key}' is runtime bookkeeping, not a JS property, got {rendered}"
    );
}

#[test]
fn inspect_prints_ordinary_own_keys() {
    let rendered = inspect_object_with_keys(&["alpha"]);
    assert!(
        rendered.contains("alpha"),
        "an ordinary own key must be printed, got {rendered}"
    );
}

#[test]
fn inspect_hides_the_constructing_class_pin() {
    assert_hidden_from_inspect("__perry_ctor_class_object");
}

#[test]
fn inspect_hides_the_class_object_parent_edge() {
    assert_hidden_from_inspect("__perry_parent_class");
}
