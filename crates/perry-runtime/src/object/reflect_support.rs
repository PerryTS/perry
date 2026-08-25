//! Reflect-specific support predicates (#2756/#2758/#2760/#2762).
//!
//! These helpers expose just enough of an object's recorded metadata
//! (extensibility flag, own-key presence, per-property writable/configurable
//! attributes) for `crate::proxy`'s `Reflect.*` entry points to compute the
//! correct boolean results that Node returns — without the Reflect code
//! reaching into object internals directly. Split out of `object_ops.rs` to
//! keep that file under the 2000-line lint cap.

use super::object_ops::{extract_obj_ptr, gc_header_for};

/// Is `value` a heap object that codegen would treat as a target? Returns
/// `false` for primitives, null/undefined, class refs, and other non-pointer
/// tags. Used by `Reflect.preventExtensions` / `Reflect.isExtensible` to throw
/// a `TypeError` on non-object targets (whereas the `Object.*` helpers tolerate
/// them).
pub(crate) fn js_value_is_heap_object(value: f64) -> bool {
    unsafe { !extract_obj_ptr(value).is_null() }
}

/// Does the heap object behind `value` currently carry the `OBJ_FLAG_NO_EXTEND`
/// flag? Returns `false` for non-objects.
pub(crate) fn obj_value_no_extend(value: f64) -> bool {
    unsafe {
        let obj = extract_obj_ptr(value);
        if obj.is_null() || (obj as usize) <= 0x10000 {
            return false;
        }
        // Typed arrays use a side table (small ones carry no `GcHeader`, so
        // the header read below would be allocator-metadata garbage).
        if crate::typedarray::lookup_typed_array_kind(obj as usize).is_some() {
            return crate::typedarray_props::typed_array_owner_no_extend(obj as usize);
        }
        let gc = gc_header_for(obj);
        (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0
    }
}

/// Does the heap object behind `value` have an own (string-keyed) property
/// named `key`? Used to distinguish "define a new property on a non-extensible
/// object" (fails) from "redefine an existing one" (may succeed). Symbol keys
/// are resolved through the symbol side-table.
pub(crate) fn obj_value_has_own_key(value: f64, key: f64) -> bool {
    unsafe {
        if crate::symbol::js_is_symbol(key) != 0 {
            // Presence cannot be inferred from the value: an own Symbol-keyed
            // data property is allowed to contain `undefined`.  The old value
            // probe therefore turned an existing writable property into an
            // apparent miss, which made OrdinarySet drop the first metadata
            // overwrite on native AsyncResource handles.
            return crate::symbol::has_own_symbol_property(value, key);
        }
        let obj = extract_obj_ptr(value);
        if obj.is_null() {
            return false;
        }
        let obj_addr = obj as usize;
        // TypedArray FIRST: own keys are the valid integer indices plus the
        // expando side table. Must precede the GC-header read below — small
        // typed arrays are plain-`alloc`ed without a `GcHeader`, so reading
        // `addr - 8` is allocator-metadata garbage.
        if crate::typedarray::lookup_typed_array_kind(obj_addr).is_some() {
            // #6943: `js_string_coerce` allocates for every non-heap-string key
            // and runs a user `toString` / `valueOf` for an object key, so it
            // can trigger a GC that **evacuates**. `obj` was resolved from
            // `value` before the call and is dereferenced as a
            // `TypedArrayHeader` after it.
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);
            let key_str = crate::builtins::js_string_coerce(key);
            let obj = obj_handle.get_raw_mut_ptr::<super::ObjectHeader>();
            if key_str.is_null() {
                return false;
            }
            return crate::typedarray_props::typed_array_has_own_property(
                obj as *const crate::typedarray::TypedArrayHeader,
                key_str,
            );
        }
        // Buffer / ArrayBuffer / DataView NEXT, and for the same reason
        // (#8117). These receivers are `BufferHeader`s, not `ObjectHeader`s:
        // they have no `class_id` and no `keys_array`. Nothing below rejected
        // them, so the ordinary arm read `crate::object::object_keys_array(obj)` out of the bytes
        // that follow a buffer header, and handed that to `js_array_length` —
        // which dereferences `addr - 8` for its lazy-array probe. The only
        // thing between the two was a `< 0x10000` magnitude floor, which
        // arbitrary payload bytes clear routinely:
        //
        //     const b: any = Buffer.alloc(8);
        //     b.readUInt8 = function () { return "shadowed"; };
        //     const k = "readUInt8";
        //     b[k](0);                       // SIGSEGV, 10/10 on Linux
        //
        // reached through `js_put_value_set_dyn_ic_miss` ->
        // `proxy::ordinary_set_with_receiver` -> `proxy::own_set_descriptor`.
        // It is the same "ask the receiver question before the generic walk
        // claims it" shape as #8090/#8109/#8119/#8120, on the has-own-key path.
        //
        // A buffer's OWN string keys are exactly its expando table (#6406).
        // Prototype methods (`readUInt8`, `subarray`, …) are inherited, not
        // own, so they must answer false — that is what lets `b.readUInt8 = fn`
        // install a shadowing own property instead of being treated as a
        // redefinition. Canonical integer indices are deliberately NOT folded
        // in: the byte-index `[[Set]]` is routed upstream of this call, and
        // answering "own" for one would divert it into the ordinary
        // data-property store.
        if crate::buffer::is_registered_buffer(obj_addr) {
            // `key_to_rust_string` runs `js_string_coerce`, which allocates and
            // can therefore evacuate. The buffer's address is the side-table
            // KEY, so carry it across the call on a handle rather than binding
            // the pre-call value (#6943).
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);
            let (key_name, obj) =
                obj_handle.across_mut::<super::ObjectHeader, _>(|| key_to_rust_string(key));
            let Some(key_name) = key_name else {
                return false;
            };
            return crate::buffer::buffer_has_own_prop(obj as usize, &key_name);
        }
        if obj_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc = gc_header_for(obj);
            if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
                || (*gc).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                let arr = crate::array::clean_arr_ptr(obj as *const crate::array::ArrayHeader);
                if arr.is_null() {
                    return false;
                }
                // #6943: `arr` is the (tag-cleaned) array header, resolved
                // before the GC-capable coercion and walked after it.
                let scope = crate::gc::RuntimeHandleScope::new();
                let arr_handle = scope.root_raw_const_ptr(arr);
                let key_str = crate::builtins::js_string_coerce(key);
                let arr = arr_handle.get_raw_const_ptr::<crate::array::ArrayHeader>();
                if key_str.is_null() {
                    return false;
                }
                return super::has_own_helpers::array_own_key_present(arr, key_str);
            }
        }
        if crate::closure::is_closure_ptr(obj_addr) {
            let Some(key_name) = key_to_rust_string(key) else {
                return false;
            };
            return super::has_own_helpers::closure_own_key_present(obj_addr, &key_name);
        }
        // Native-module namespaces (console, fs, …) expose their members as
        // VIRTUAL keys — dispatch tables, not keys_array entries. Mirror the
        // `js_object_get_own_property_descriptor` arm so a redefinition like
        // `Object.defineProperty(console, 'error', { value })` (Next.js
        // patches console methods this way, repeatedly) is treated as
        // redefining an EXISTING property — absent descriptor attributes then
        // retain the property's writable/enumerable/configurable=true
        // defaults instead of collapsing to the new-property `false`s (which
        // made the SECOND patch throw `Cannot redefine property`).
        if (*obj).class_id == super::native_module::NATIVE_MODULE_CLASS_ID {
            // Armed ops table (see `nm_namespace_hooks`): keeps the virtual
            // key tables out of binaries with no module imports. Unarmed +
            // matching class_id is unreachable (only the arming bootstrap
            // assigns the class id).
            if let Some(ops) = super::nm_namespace_ops() {
                if (ops.reflect_has_enumerable)(obj, key) {
                    return true;
                }
            }
        }
        // #6943: the ordinary arm dereferences `obj` for its `keys_array`
        // *after* the GC-capable coercion, so the receiver is rooted across it.
        // (#7963 dropped the `string_coerce_is_inert` shortcut around this
        // scope: the keys walk below needs the same scope for its own roots
        // whatever the key's shape, so skipping it here bought nothing.)
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        let (key_str, obj) = obj_handle
            .across_mut::<super::ObjectHeader, _>(|| crate::builtins::js_string_coerce(key));
        if key_str.is_null() {
            return false;
        }
        if let Some(present) = crate::process::process_env_has_field(obj, key_str) {
            return present;
        }
        // #7963 (the second half of #6949's deferred scope note): `js_array_get`
        // MATERIALIZES a lazy array, so it can allocate and therefore evacuate.
        // `keys` and `key_str` were raw Rust locals walked across it — neither
        // shadow slots nor temp roots nor reachable from any registered
        // scanner, so the collector could neither keep them alive nor rewrite
        // them, and the very next iteration compared a from-space string
        // against a from-space slot. Root both and re-read each iteration; the
        // pre-call addresses are never bound past the call.
        let keys_handle = scope.root_raw_mut_ptr(crate::object::object_keys_array(obj));
        let key_handle = scope.root_string_ptr(key_str);
        let ((), mut keys) = keys_handle.across_mut::<crate::array::ArrayHeader, _>(|| ());
        // Defence in depth for the class the buffer arm above closes by
        // routing: `keys_array` is only an `ArrayHeader` when `obj` really is
        // an `ObjectHeader`, and a receiver kind with no arm here reaches this
        // line holding payload bytes. A bare magnitude floor does not catch
        // that — use the canonical predicate, which rejects the handle band and
        // anything outside the heap before `js_array_length` dereferences
        // `keys - 8`. A missing arm should be a wrong answer, not a SIGSEGV.
        if keys.is_null() || !crate::value::addr_class::is_plausible_heap_addr(keys as usize) {
            return false;
        }
        let key_count = crate::array::js_array_length(keys) as usize;
        for i in 0..key_count {
            let (stored, refreshed_keys) =
                keys_handle.across_mut::<crate::array::ArrayHeader, _>(|| {
                    crate::array::js_array_get(keys, i as u32)
                });
            let ((), key_str) = key_handle.across_const::<crate::StringHeader, _>(|| ());
            keys = refreshed_keys;
            if crate::string::js_string_key_matches(stored, key_str) {
                return true;
            }
        }
        false
    }
}

/// Look up the writable/configurable attributes Perry has recorded for
/// `(value, key)`. Returns `None` when no descriptor has been installed (the JS
/// default of all-true applies). The booleans are `(writable, configurable)`.
pub(crate) fn obj_value_attrs(value: f64, key: f64) -> Option<(bool, bool)> {
    unsafe {
        let obj = extract_obj_ptr(value);
        if obj.is_null() {
            return None;
        }
        // #6943: `key_to_rust_string` runs the GC-capable `js_string_coerce`,
        // and `obj as usize` is the descriptor side table's OWNER KEY. A stale
        // address doesn't crash here — it silently misses, so a
        // `Reflect.defineProperty` on a non-configurable property would report
        // the all-true default and let the redefine through. Root the receiver
        // across the coercion. (Not in #6943's site list; found by reading.)
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj_handle = scope.root_raw_mut_ptr(obj);
        let k = key_to_rust_string(key)?;
        let obj = obj_handle.get_raw_mut_ptr::<super::ObjectHeader>();
        super::get_property_attrs(obj as usize, &k).map(|a| (a.writable(), a.configurable()))
    }
}

#[inline]
fn reflect_bool(b: bool) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(b).bits())
}

/// Ordinary (non-proxy) `Reflect.defineProperty` `[[DefineOwnProperty]]`,
/// reporting success as a NaN-boxed boolean. Shared by `crate::proxy`'s
/// `Reflect.defineProperty` entry point (both the no-trap and direct paths).
pub(crate) fn reflect_define_property(obj: f64, key: f64, descriptor: f64) -> f64 {
    // #8507: each exotic probe below may coerce `key`, which can run user JS
    // and evacuate all three operands. A helper's private handle scope keeps
    // its arguments current only for that helper; when it returns
    // `NotTypedArray` / `None`, the copies in this caller would still name
    // from-space. Keep one caller-owned set of roots and re-read it before
    // every subsequent operation.
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_heap_word_u64(obj.to_bits());
    let key_handle = scope.root_nanbox_f64(key);
    let descriptor_handle = scope.root_nanbox_f64(descriptor);

    // TypedArrays are Integer-Indexed exotic objects: a canonical numeric index
    // key returns true/false here rather than going through the ordinary object
    // machinery (which would mishandle in-bounds element writes and treats the
    // view as non-extensible).
    match unsafe {
        super::typed_array_define_own_property(
            f64::from_bits(obj_handle.get_heap_word_u64()),
            key_handle.get_nanbox_f64(),
            descriptor_handle.get_nanbox_f64(),
        )
    } {
        super::TypedArrayDefineOutcome::Defined => return reflect_bool(true),
        super::TypedArrayDefineOutcome::Rejected => return reflect_bool(false),
        super::TypedArrayDefineOutcome::NotTypedArray => {}
    }
    // The array exotic `[[DefineOwnProperty]]` for `length` (ArraySetLength)
    // reports success/failure as a boolean here rather than throwing — bypass
    // the generic non-configurable pre-check below, which would mishandle the
    // (non-configurable but writable) `length` property.
    if let Some(ok) = unsafe {
        super::array_length_reflect_define(
            f64::from_bits(obj_handle.get_heap_word_u64()),
            key_handle.get_nanbox_f64(),
            descriptor_handle.get_nanbox_f64(),
        )
    } {
        return reflect_bool(ok);
    }
    let has_own = obj_value_has_own_key(
        f64::from_bits(obj_handle.get_heap_word_u64()),
        key_handle.get_nanbox_f64(),
    );
    // Redefining a non-configurable existing property fails.
    if has_own {
        if let Some((_writable, configurable)) = obj_value_attrs(
            f64::from_bits(obj_handle.get_heap_word_u64()),
            key_handle.get_nanbox_f64(),
        ) {
            if !configurable {
                return reflect_bool(false);
            }
        }
    } else if obj_value_no_extend(f64::from_bits(obj_handle.get_heap_word_u64())) {
        // Defining a brand-new property on a non-extensible object fails.
        return reflect_bool(false);
    }
    super::js_object_define_property(
        f64::from_bits(obj_handle.get_heap_word_u64()),
        key_handle.get_nanbox_f64(),
        descriptor_handle.get_nanbox_f64(),
    );
    reflect_bool(true)
}

pub(crate) unsafe fn key_to_rust_string(value: f64) -> Option<String> {
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let name_len = (*key_str).byte_len as usize;
    std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
        .ok()
        .map(|s| s.to_string())
}

/// "Existing own key" probe for native-module namespace objects (extracted
/// verbatim from the former inline branch). Reached ONLY through
/// `NmNamespaceOps::reflect_has_enumerable`.
pub(crate) unsafe fn nm_reflect_has_enumerable(obj: *mut super::ObjectHeader, key: f64) -> bool {
    if let (Some(module_name), Some(key_name)) = (
        super::native_module::read_native_module_name(obj),
        key_to_rust_string(key),
    ) {
        if super::native_module::native_module_has_enumerable_key(&module_name, &key_name) {
            return true;
        }
    }
    false
}
