use super::super::*;
use super::disposal::*;
use super::object_proto::*;
use super::proto_dispatch::*;
use super::typed_array::*;
use super::*;

pub(super) unsafe fn dispatch_map_set(
    root_scope: &crate::gc::RuntimeHandleScope,
    object_handle: &crate::gc::RuntimeHandle,
    arg_handles: &[crate::gc::RuntimeHandle],
    object: f64,
    method_name: &str,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let jsval = JSValue::from_bits(object.to_bits());
    let raw_bits = object.to_bits();
    let refreshed_args = || crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
    let _ = (root_scope, object_handle, &refreshed_args, raw_bits, jsval);
    let _ = (method_name_ptr, method_name_len);
    // Check Map/Set registries for raw or NaN-boxed pointers.
    // Maps/Sets are allocated with plain alloc (no GcHeader), so they can't be
    // dispatched through the ObjectHeader path below.
    {
        let check_ptr = if jsval.is_pointer() {
            (raw_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if !object.is_nan()
            && crate::value::addr_class::is_above_handle_band(raw_bits as usize)
            && (raw_bits >> 48) == 0
        {
            raw_bits as usize
        } else {
            0
        };
        if check_ptr >= 0x10000 {
            if crate::map::is_registered_map(check_ptr) {
                let map = check_ptr as *mut crate::map::MapHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return Some(match method_name {
                    "get" if !args.is_empty() => crate::map::js_map_get(map, args[0]),
                    "set" if args.len() >= 2 => {
                        let result = crate::map::js_map_set(map, args[0], args[1]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => {
                        let r = crate::map::js_map_has(map, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "delete" if !args.is_empty() => {
                        let r = crate::map::js_map_delete(map, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "clear" => {
                        crate::map::js_map_clear(map);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    "size" => crate::map::js_map_size(map) as f64,
                    // #2856: value-level iterator methods return real iterator
                    // OBJECTS (not arrays), dispatched via class id.
                    "entries" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_map_entries_iter_obj(map) as *mut u8,
                        )
                        .bits(),
                    ),
                    "keys" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_map_keys_iter_obj(map) as *mut u8
                        )
                        .bits(),
                    ),
                    "values" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_map_values_iter_obj(map) as *mut u8,
                        )
                        .bits(),
                    ),
                    "forEach" if !args.is_empty() => {
                        let this_arg = args
                            .get(1)
                            .copied()
                            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                        crate::map::js_map_foreach(map, args[0], this_arg);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                });
            }
            if crate::set::is_registered_set(check_ptr) {
                let set = check_ptr as *mut crate::set::SetHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return Some(match method_name {
                    "add" if !args.is_empty() => {
                        let result = crate::set::js_set_add(set, args[0]);
                        f64::from_bits(JSValue::pointer(result as *mut u8).bits())
                    }
                    "has" if !args.is_empty() => {
                        let r = crate::set::js_set_has(set, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "delete" if !args.is_empty() => {
                        let r = crate::set::js_set_delete(set, args[0]);
                        f64::from_bits(JSValue::bool(r != 0).bits())
                    }
                    "clear" => {
                        crate::set::js_set_clear(set);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    "size" => crate::set::js_set_size(set) as f64,
                    // #2856: dynamic Set iterator methods previously fell
                    // through to `undefined` (only add/has/delete/clear/size
                    // were handled). Return real iterator objects; `entries`
                    // yields `[v, v]` pairs.
                    "values" | "keys" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_set_values_iter_obj(set) as *mut u8,
                        )
                        .bits(),
                    ),
                    "entries" => f64::from_bits(
                        JSValue::pointer(
                            crate::collection_iter_object::js_set_entries_iter_obj(set) as *mut u8,
                        )
                        .bits(),
                    ),
                    "forEach" if !args.is_empty() => {
                        let this_arg = args
                            .get(1)
                            .copied()
                            .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
                        crate::set::js_set_foreach(set, args[0], this_arg);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    // #2872: ES2024 Set composition methods. union/intersection/
                    // difference/symmetricDifference return a new Set; the
                    // is* predicates return a boolean.
                    "union" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(crate::set::js_set_union(set, args[0]) as *mut u8).bits(),
                    ),
                    "intersection" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(crate::set::js_set_intersection(set, args[0]) as *mut u8)
                            .bits(),
                    ),
                    "difference" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(crate::set::js_set_difference(set, args[0]) as *mut u8)
                            .bits(),
                    ),
                    "symmetricDifference" if !args.is_empty() => f64::from_bits(
                        JSValue::pointer(
                            crate::set::js_set_symmetric_difference(set, args[0]) as *mut u8
                        )
                        .bits(),
                    ),
                    "isSubsetOf" if !args.is_empty() => f64::from_bits(
                        JSValue::bool(crate::set::js_set_is_subset_of(set, args[0]) != 0).bits(),
                    ),
                    "isSupersetOf" if !args.is_empty() => f64::from_bits(
                        JSValue::bool(crate::set::js_set_is_superset_of(set, args[0]) != 0).bits(),
                    ),
                    "isDisjointFrom" if !args.is_empty() => f64::from_bits(
                        JSValue::bool(crate::set::js_set_is_disjoint_from(set, args[0]) != 0)
                            .bits(),
                    ),
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                });
            }
            // Buffer / Uint8Array dispatch — allocated raw, not behind a
            // GcHeader, so it can't be discovered through the ObjectHeader
            // path below. Tracked in BUFFER_REGISTRY. Routes Node-style
            // numeric read/write/search/swap method family through
            // `crate::buffer` helpers.
            if crate::buffer::is_registered_buffer(check_ptr) {
                return Some(dispatch_buffer_method(
                    check_ptr,
                    method_name,
                    args_ptr,
                    args_len,
                ));
            }
        }
    }

    None
}

pub(super) unsafe fn dispatch_raw_pointer(
    root_scope: &crate::gc::RuntimeHandleScope,
    object_handle: &crate::gc::RuntimeHandle,
    arg_handles: &[crate::gc::RuntimeHandle],
    object: f64,
    method_name: &str,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let jsval = JSValue::from_bits(object.to_bits());
    let raw_bits = object.to_bits();
    let refreshed_args = || crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(arg_handles);
    let _ = (root_scope, object_handle, &refreshed_args, raw_bits, jsval);
    let _ = (method_name_ptr, method_name_len);
    // Handle raw pointer values without NaN-box tags.
    // Perry sometimes bitcasts I64 pointers to F64 without NaN-boxing (POINTER_TAG).
    // These appear as subnormal floats with bits in the valid heap address range
    // (above the handle band, below 0x0000_FFFF_FFFF_FFFF, upper 16 bits = 0).
    if !jsval.is_pointer()
        && !object.is_nan()
        && crate::value::addr_class::is_above_handle_band(raw_bits as usize)
        && (raw_bits >> 48) == 0
    {
        // Looks like a raw heap pointer — re-wrap as POINTER_TAG and retry
        let reboxed = f64::from_bits(0x7FFD_0000_0000_0000u64 | raw_bits);
        let reboxed_jsval = JSValue::from_bits(reboxed.to_bits());
        let obj = reboxed_jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader before accessing
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            // Check for native module namespace
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                // #853: same dead-after-return as the first arm above.
                return Some(
                    crate::object::native_module::call_native_module_dispatch_hook(
                        obj,
                        method_name,
                        args_ptr,
                        args_len,
                    ),
                );
            }
            // Issue #1206: same class-id check as the NaN-boxed path above
            // so a raw-pointer iterator value (uncommon, but possible after
            // a bitcast) still routes through the iterator dispatcher.
            if (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID {
                return Some(crate::buffer::dispatch_buffer_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            // #321: same array-iterator class-id check as the NaN-boxed path.
            if (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return Some(crate::array::dispatch_array_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            // #2856: same Map/Set-iterator class-id checks as the NaN-boxed path.
            if (*obj).class_id == crate::collection_iter_object::MAP_ITERATOR_CLASS_ID {
                return Some(crate::collection_iter_object::dispatch_map_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            if (*obj).class_id == crate::collection_iter_object::SET_ITERATOR_CLASS_ID {
                return Some(crate::collection_iter_object::dispatch_set_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            if (*obj).class_id == crate::string::STRING_ITERATOR_CLASS_ID {
                return Some(crate::string::dispatch_string_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            #[cfg(feature = "regex-engine")]
            if (*obj).class_id == crate::regex::REGEXP_STRING_ITERATOR_CLASS_ID {
                return Some(crate::regex::dispatch_regexp_string_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            // #2874: lazy iterator-helper objects, same as the NaN-boxed path.
            if (*obj).class_id == crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID {
                return Some(crate::iterator_helpers::dispatch_iterator_helper_method(
                    obj as *mut ObjectHeader,
                    method_name,
                    args_ptr,
                    args_len,
                ));
            }

            // Field name scan on this object
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_bytes = method_name.as_bytes();
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if crate::string::js_string_key_matches_bytes(key_val, method_bytes) {
                                let field_val = js_object_get_field(obj as *mut _, i as u32);
                                if field_val.is_pointer() {
                                    return Some(crate::closure::js_native_call_value(
                                        f64::from_bits(field_val.bits()),
                                        args_ptr,
                                        args_len,
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = raw_bits as i64;
                    return Some(call_vtable_method(
                        func_ptr,
                        this_i64,
                        args_ptr,
                        args_len,
                        param_count,
                        has_synthetic_arguments,
                        has_rest,
                    ));
                }
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        // Refs #420: parent-chain walk (mirror of the path
                        // above for raw pointer instances).
                        let mut cur_cid = class_id;
                        let mut depth = 0u32;
                        while depth < 32 {
                            if let Some(vtable) = reg.get(&cur_cid) {
                                if let Some(entry) = vtable.methods.get(method_name) {
                                    vtable_ic_insert(
                                        class_id,
                                        method_name_ptr as usize,
                                        entry.func_ptr,
                                        entry.param_count,
                                        entry.has_synthetic_arguments,
                                        entry.has_rest,
                                    );
                                    let this_i64 = raw_bits as i64;
                                    return Some(call_vtable_method(
                                        entry.func_ptr,
                                        this_i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                        entry.has_synthetic_arguments,
                                        entry.has_rest,
                                    ));
                                }
                            }
                            match get_parent_class_id(cur_cid) {
                                Some(pid) if pid != 0 => {
                                    cur_cid = pid;
                                    depth += 1;
                                }
                                _ => break,
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
