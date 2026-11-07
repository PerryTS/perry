//! The namespace-object `NativeModuleVtable` entries for own-field reads
//! (`vt_get_own_field`) and own-key enumeration (`vt_own_keys_array`). Split
//! out of `object/native_module.rs` to stay under the 2,000-line cap (#10750);
//! the vtable itself and its install path stay in the parent.

use super::*;

// ─── Vtable impls relocated from field_get_set.rs (EN size work) ───────
// Bodies moved verbatim so their table references are reachable only
// through the installed vtable. See `NativeModuleVtable`.

/// Own-field read on a namespace object (`fs.constants`, method values,
/// process IPC props, …). Returns `None` when the receiver carries no
/// module name — the caller falls through to the generic field scan.
pub(super) unsafe fn vt_get_own_field(
    obj: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    let key_ptr = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let key_len = (*key).byte_len as usize;
    let nb_ptr = crate::value::js_nanbox_pointer(obj as i64);
    let module = js_object_get_field(obj as *mut _, 0);
    // SSO-aware (#1781): a module name of five bytes or fewer (`fs`, `os`,
    // `path`) is stored inline, not as a heap string.
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if (*obj).class_id != NATIVE_MODULE_CLASS_ID
        || !crate::string::js_string_key_bytes(module, &mut sso_buf).is_some_and(|b| !b.is_empty())
    {
        return None;
    }
    // Getters can throw: enter them before owning the module-name String.
    if let Some(value) = super::field_get_set::own_accessor_get_by_bytes(
        obj,
        std::slice::from_raw_parts(key_ptr, key_len),
        nb_ptr,
    ) {
        return Some(value);
    }
    let module_name = get_module_name_from_namespace(nb_ptr);
    if module_name.is_empty() {
        return None;
    }
    let property_name =
        std::str::from_utf8(std::slice::from_raw_parts(key_ptr, key_len)).unwrap_or("");
    // A deleted member is no own property: the read skips
    // the module's tables and continues at the namespace's `[[Prototype]]`.
    'own: {
        // A user override (`require('node:timers').setImmediate = patched`)
        // wins all built-in resolution below — CJS exports are mutable in Node.
        // (A worker-hidden `process` member reads `undefined` below, through
        // `process_ipc_property` / its bound export.)
        match native_namespace_prop_override_bits(&module_name, property_name) {
            Some(DELETED) => break 'own,
            Some(bits) => return Some(JSValue::from_bits(bits)),
            None => {}
        }
        if matches!(
            module_name.as_str(),
            "process" | "process.namespace" | "process.default"
        ) {
            if let Some(value) = crate::process::process_ipc_property(property_name) {
                return Some(JSValue::from_bits(value.to_bits()));
            }
        }
        if let Some(value) = super::field_get_set::native_module_own_field_by_key(obj, key) {
            return Some(value);
        }
        if let Some(value) = performance_namespace_method(&module_name, property_name, nb_ptr) {
            return Some(JSValue::from_bits(value.to_bits()));
        }
        // #3687: node:cluster default-import EventEmitter methods on the
        // distinct `cluster.default` namespace (see original comment at the
        // pre-relocation site in field_get_set.rs history).
        if module_name == "cluster.default" && super::is_cluster_emitter_method(property_name) {
            return Some(JSValue::from_bits(
                bound_native_callable_export_value(&module_name, property_name).to_bits(),
            ));
        }
        if let Some(val) = get_native_module_constant(&module_name, property_name, nb_ptr) {
            return Some(JSValue::from_bits(val.to_bits()));
        }
        if module_name == "crypto.webcrypto" {
            if let Some(value) = super::global_this::webcrypto_method_value(property_name) {
                return Some(JSValue::from_bits(value.to_bits()));
            }
        }
        if module_name == "crypto.subtle" {
            if let Some(value) = super::global_this::subtle_crypto_method_value(property_name) {
                return Some(JSValue::from_bits(value.to_bits()));
            }
        }
        // Issue #894: callable exports (`("events", "EventEmitter")` …) get a
        // bound-method closure for require-then-member-access parity.
        if is_native_module_callable_export(&module_name, property_name) {
            if let Some(bound) = instance_bound_perf_method(&module_name, property_name, nb_ptr) {
                return Some(JSValue::from_bits(bound.to_bits()));
            }
            let value = bound_native_callable_export_value(&module_name, property_name);
            let value = if module_name == "bun" && property_name == "hash" {
                crate::bun_compat::decorate_bun_hash(value)
            } else {
                value
            };
            return Some(JSValue::from_bits(value.to_bits()));
        }
        // Object-valued exports (e.g. `perf_hooks.performance` / `.constants`) are
        // resolved by the shared per-property dispatch but are not covered by the
        // override / constant / callable checks above. Without delegating, a DYNAMIC
        // namespace read (`createRequire(...)("perf_hooks").performance`,
        // `process.getBuiltinModule(...)`) returned undefined for them while the
        // static codegen path resolved them via `js_native_module_property_by_name`.
        // Defer to that authoritative resolver so dynamic namespaces match static.
        if native_module_has_enumerable_key(&module_name, property_name) {
            let resolved = js_native_module_property_by_name(
                module_name.as_ptr(),
                module_name.len(),
                key_ptr,
                key_len,
            );
            return Some(JSValue::from_bits(resolved.to_bits()));
        }
    }
    // #11542: not an own property — continue at the namespace's
    // `[[Prototype]]` (see `namespace_prototype`).
    Some(namespace_prototype::non_own_field(
        obj,
        key,
        &module_name,
        key_ptr,
        key_len,
    ))
}

/// `Object.keys(namespace)` — fresh array of the module's enumerable
/// keys. `None` when the module is unknown; caller falls back to the
/// generic keys_array path. Also reused by `Object.getOwnPropertyNames`
/// (#5268): a native-module object must enumerate its export surface there
/// too, not the internal `__module__` sentinel.
pub(crate) unsafe fn vt_own_keys_array(
    obj: *const ObjectHeader,
) -> Option<*mut crate::array::ArrayHeader> {
    // This hook is also offered to arbitrary values by getOwnPropertyNames.
    // Read the namespace brand from the shape before interpreting field zero
    // as a module name; an ordinary object's first value is just user data.
    let header = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if crate::gc_type_is_not_object!(header.obj_type)
        || super::shapes::object_shape_descriptor(obj)?.object_kind
            != super::shapes::ShapeObjectKind::NativeNamespace
    {
        return None;
    }
    let module_name = read_native_module_name(obj)?;
    let keys = native_module_enumerable_keys(&module_name)?;
    let include_permission = matches!(
        module_name.as_str(),
        "process" | "process.namespace" | "process.default"
    ) && crate::process::process_permission_enabled();
    let out = crate::array::js_array_alloc(keys.len() as u32 + include_permission as u32);
    for key_bytes in keys {
        if native_namespace_member_absent(
            &module_name,
            std::str::from_utf8(key_bytes).unwrap_or(""),
        ) {
            continue;
        }
        let key_str =
            crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
        crate::array::js_array_push(out, JSValue::string_ptr(key_str));
    }
    if include_permission {
        let key_str =
            crate::string::js_string_from_bytes(b"permission".as_ptr(), b"permission".len() as u32);
        crate::array::js_array_push(out, JSValue::string_ptr(key_str));
    }
    Some(out)
}
