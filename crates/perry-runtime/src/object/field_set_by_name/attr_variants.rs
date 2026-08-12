//! `js_object_set_field_by_name` variants that install a non-default property
//! attribute after the ordinary store (non-enumerable for derived-class
//! `super(message)`, non-configurable for `globalThis` declaration bindings).
//! Split out of `object/field_set_by_name.rs` (issue #7402) — pure
//! relocation, no logic changes.

use super::*;

/// Set `obj[key] = value` as a non-enumerable (but writable + configurable)
/// own data property. Used by derived-class `super(message)` into a built-in
/// `Error`/`NativeError`: the spec sets `message` via DefinePropertyOrThrow
/// with `{ writable: true, enumerable: false, configurable: true }`, whereas an
/// ordinary assignment would create an enumerable property. (Test262
/// subclass/.../NativeError/*-message.)
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name_nonenum(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    js_object_set_field_by_name(obj, key, value);
    // Only ordinary heap objects carry the attrs side-table. Class refs,
    // TypedArrays, Temporal cells, etc. are handled by `set_field_by_name`'s own
    // routing and never reach the ordinary enumerable default, so skip them.
    let bits = obj as u64;
    if (bits >> 48) == 0x7FFE
        || crate::value::addr_class::is_handle_band(obj as usize)
        || key.is_null()
    {
        return;
    }
    unsafe {
        if !crate::object::is_valid_obj_ptr(obj as *const u8) {
            return;
        }
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            crate::object::set_property_attrs(
                obj as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
}

/// Set `obj[key] = value` as a non-configurable (but writable + enumerable)
/// own data property. Used for `globalThis`'s reflection of a Script's
/// top-level `function`/`var` declarations: GlobalDeclarationInstantiation's
/// `CreateGlobalFunctionBinding`/`CreateGlobalVarBinding` call with `D = false`
/// (unlike sloppy-eval's Annex B.3.3.3 path, which uses `D = true` and already
/// has its own `Object.defineProperty` HIR synthesis in
/// `global_eval_hoist.rs`) — an ordinary assignment would instead create a
/// configurable property. Called once at program start before any user
/// statement runs, so there's no pre-existing descriptor to preserve or
/// `CanDeclareGlobalFunction`/`CanDeclareGlobalVar` extensibility check to
/// replicate here (test262 `language/global-code/decl-func.js`).
#[no_mangle]
pub extern "C" fn js_object_set_field_by_name_nonconfigurable(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    js_object_set_field_by_name(obj, key, value);
    let bits = obj as u64;
    if (bits >> 48) == 0x7FFE
        || crate::value::addr_class::is_handle_band(obj as usize)
        || key.is_null()
    {
        return;
    }
    unsafe {
        if !crate::object::is_valid_obj_ptr(obj as *const u8) {
            return;
        }
        let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key).byte_len as usize;
        if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
            crate::object::set_property_attrs(
                obj as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, true, false),
            );
        }
    }
}
