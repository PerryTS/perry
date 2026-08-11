//! Key/diagnostic utilities and the non-plain-receiver write routines used
//! by `js_object_set_field_by_name` (closure receivers, the #6530 class-object
//! static mirror, and the native-module namespace override). Split out of
//! `object/field_set_by_name.rs` (issue #7402) — pure relocation, no logic
//! changes; the four previously file-private helpers are now `pub(super)` so
//! the sibling modules of the split can reach them.

use super::*;

/// Issue #615 helper — read a `*const StringHeader` as a Rust `String`
/// for inclusion in TypeError diagnostic messages. Returns `"<unknown>"`
/// for null / non-UTF-8 / corrupt headers so the throw still fires
/// rather than panicking on the slow-path edge case.
pub(super) unsafe fn key_to_str_for_diag(key: *const crate::StringHeader) -> String {
    if key.is_null() {
        return "<unknown>".to_string();
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    if name_len == 0 {
        return String::new();
    }
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    std::str::from_utf8(name_bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string())
}

pub(super) unsafe fn string_key_eq(key: *const crate::StringHeader, expected: &[u8]) -> bool {
    if key.is_null() || (key as usize) < 0x10000 {
        return false;
    }
    let len = (*key).byte_len as usize;
    if len != expected.len() {
        return false;
    }
    let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    std::slice::from_raw_parts(data, len) == expected
}

/// Shared closure-receiver named-property WRITE path — used by both ways a
/// closure is recognized in `js_object_set_field_by_name` (a `GC_TYPE_CLOSURE`
/// GcHeader-typed pointer, and the raw `CLOSURE_MAGIC`-tagged fallback for a
/// pointer reached without a full GC header). #3143: honors a non-writable
/// registered descriptor (a built-in method's `.name`/`.length` are spec'd
/// `writable: false`); `Object.defineProperty(Function.prototype, k, {...})`
/// round-trips via `closure_set_via_function_prototype_descriptor` before
/// falling back to a plain own-property write.
/// #6530: mirror a SUCCESSFUL own-data write on a per-evaluation CLASS OBJECT
/// (`object_type == OBJECT_TYPE_CLASS` — what a capture-carrying class
/// statement materializes as) into the class_id-keyed `CLASS_DYNAMIC_PROPS`
/// side table. Compiled method bodies reference sibling classes as INT32
/// ClassRefs (bundled zod's `ZodOptional.create(this, this._def)` inside
/// `ZodType.optional()`), and `js_class_static_method_call` resolves statics
/// through that table only — without the mirror the dispatch missed and
/// handed back the class ref itself, so `.optional()` returned the
/// ZodOptional CLASS instead of an instance.
///
/// Called ONLY at the own-data write completions in
/// `js_object_set_field_by_name` (after the accessor walk, frozen/sealed
/// gates, and writable checks have all passed), so a setter-intercepted or
/// rejected assignment never desyncs the ClassRef read path from the class
/// object's real state. Internal `__perry_*` markers (the pinned-parent
/// edge) stay object-local. Last-wins across evaluations of the same class
/// statement, matching the established template-cid compromise.
pub(super) unsafe fn mirror_class_object_static_write(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if (*obj).object_type != crate::error::OBJECT_TYPE_CLASS
        || (*obj).class_id == 0
        || key.is_null()
    {
        return;
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    if let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) {
        if !name.is_empty() && !name.starts_with("__perry_") {
            class_dynamic_prop_root_store((*obj).class_id, name, value);
        }
    }
}

pub(super) unsafe fn closure_set_field_by_name(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    if key.is_null() {
        return;
    }
    let name_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key).byte_len as usize;
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len);
    let Ok(name_str) = std::str::from_utf8(name_bytes) else {
        return;
    };
    // ECMAScript "poison pill" — assigning `caller`/`arguments` on any
    // strict-mode function (Perry compiles everything strict: declarations,
    // expressions, bound and built-in closures, arrows) throws via the
    // %ThrowTypeError% accessor's missing setter. A genuine own data prop of
    // that name (defineProperty round-trip) still wins.
    // Refs test262 13.2-*-s / StrictFunction_restricted-*.
    if matches!(name_str, "caller" | "arguments")
        && !crate::closure::closure_has_own_dynamic_prop(obj as usize, name_str)
    {
        crate::fs::validate::throw_type_error_with_code(
            "Restricted function property assignment",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if let Some(attrs) = super::get_property_attrs(obj as usize, name_str) {
        if !attrs.writable() {
            return;
        }
    } else if matches!(name_str, "name" | "length") {
        return;
    } else if !crate::closure::closure_has_own_dynamic_prop(obj as usize, name_str)
        && crate::closure::closure_set_via_function_prototype_descriptor(
            obj as usize,
            name_str,
            value,
            crate::value::js_nanbox_pointer(obj as i64),
        )
    {
        // Handled by an inherited %Function.prototype% descriptor
        // (`Object.defineProperty(Function.prototype, k, {...})`) — an
        // accessor's setter ran (or threw for a getter-only accessor), or a
        // non-writable data property blocked the write; no own property is
        // created.
        return;
    }
    crate::closure::closure_set_dynamic_prop(obj as usize, name_str, value);
}

/// Dynamic field store on a native-module namespace object (extracted
/// verbatim from the former inline branch). Reached ONLY through
/// `NmNamespaceOps::field_set_override`; returns true when the store was
/// fully handled (caller returns), false to fall through to the generic
/// store path.
pub(crate) unsafe fn nm_field_set_override(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let property_name =
        std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
    let module_name = get_module_name_from_namespace(crate::value::js_nanbox_pointer(obj as i64));
    if module_name == "buffer.Buffer" && property_name == "poolSize" {
        super::set_buffer_pool_size(value);
        return true;
    }
    // CommonJS module exports are MUTABLE in Node: monkey-patching
    // like Next.js's `require('node:timers').setImmediate = patched`
    // must store the override (read back via `vt_get_own_field`)
    // instead of falling through to the frozen-object throw.
    if !module_name.is_empty() && property_name != "__module__" {
        super::native_module::native_namespace_prop_override_store(
            module_name,
            property_name,
            value,
        );
        return true;
    }
    false
}
