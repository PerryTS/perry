use super::super::*;
use super::disposal::*;
use super::object_proto::*;
use super::proto_dispatch::*;
use super::typed_array::*;
use super::*;

/// Wall 10 — resolve and invoke a method that a framework attached to a native
/// registry handle via `Object.setPrototypeOf(handle, proto)` (Express's
/// augmented `res.send` / `req.accepts` / …). The link was recorded in the
/// `OBJECT_PROTOTYPES` side-table keyed by the handle id (see
/// `js_object_set_prototype_of`). Walk it via `resolve_inherited_field`; if the
/// resolved member is a callable closure, invoke it with `this` bound to the
/// handle value (`object`) so the method's internal `this.end(...)` /
/// `this.statusCode = …` route back to the native handle dispatch.
///
/// Returns `None` when no recorded prototype yields a callable for `method_name`
/// — the caller then falls back to JS `undefined`, preserving prior behavior for
/// genuinely-unknown handle methods.
unsafe fn dispatch_handle_proto_method(
    handle_id: usize,
    object: f64,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    // Only do the (locked) side-table walk when a prototype was actually
    // recorded for this handle — the overwhelmingly common case (fastify /
    // axios / ioredis handles with no user setPrototypeOf) skips it cheaply.
    crate::object::prototype_chain::object_static_prototype(handle_id)?;
    let key = crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let resolved = crate::object::prototype_chain::resolve_inherited_field(
        handle_id,
        key as *const crate::StringHeader,
    )?;
    let resolved_bits = resolved.bits();
    if (resolved_bits >> 48) != 0x7FFD {
        return None;
    }
    let closure_ptr = (resolved_bits & crate::value::POINTER_MASK) as usize;
    if closure_ptr == 0 || !crate::closure::is_closure_ptr(closure_ptr) {
        return None;
    }
    // An inherited prototype method may be a closure that BAKED `this` into a
    // capture slot at definition time (object-literal methods are lowered with
    // `captures_this`). Setting IMPLICIT_THIS alone can't override that slot, so
    // rebind the closure's `this` to the handle receiver first —
    // `clone_closure_rebind_this` is a no-op for closures that don't capture
    // `this` and for non-closure values. Mirrors the class-prototype fallback.
    let _ = closure_ptr;
    let bound = crate::closure::clone_closure_rebind_this(resolved_bits, object);
    let prev = crate::object::js_implicit_this_set(object);
    let result = crate::closure::js_native_call_value(f64::from_bits(bound), args_ptr, args_len);
    crate::object::js_implicit_this_set(prev);
    Some(result)
}

pub(super) unsafe fn dispatch_handle(
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
    // Check if this is a handle-based object (small integer, not a real heap pointer)
    // Handles are used by Fastify, ioredis, and other native modules that store
    // objects in a registry and use integer IDs to reference them.
    if jsval.is_pointer() {
        let raw_ptr = jsval.as_pointer::<u8>() as usize;
        if crate::value::addr_class::is_small_handle(raw_ptr) {
            // This is a handle, not a real memory pointer - dispatch to stdlib
            if let Some(dispatch) = handle_method_dispatch() {
                let r = dispatch(
                    raw_ptr as i64,
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                );
                // Wall 10 — when the native handle dispatch doesn't recognise the
                // method (returns `undefined`), the call may target a method that
                // a framework attached via `Object.setPrototypeOf(handle, proto)`
                // (Express's `res.send` / `req.accepts`, …). Walk the recorded
                // handle prototype; if it yields a callable, invoke it with
                // `this` bound to the handle so the method's internal
                // `this.end(...)` / `this.statusCode = …` route back to us.
                if r.to_bits() == crate::value::TAG_UNDEFINED {
                    if let Some(v) = dispatch_handle_proto_method(
                        raw_ptr,
                        object,
                        method_name,
                        args_ptr,
                        args_len,
                    ) {
                        return Some(v);
                    }
                }
                return Some(r);
            }
            // No dispatcher registered: still try a setPrototypeOf'd method.
            if let Some(v) =
                dispatch_handle_proto_method(raw_ptr, object, method_name, args_ptr, args_len)
            {
                return Some(v);
            }
            // Return JS `undefined`. Must be TAG_UNDEFINED (0x7FFC_..._0001); the
            // bit pattern 0x7FF8_..._0001 a prior copy used is a signaling NaN (a
            // JS number), which leaks out as a non-object and trips
            // `js_iterator_result_validate`.
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }

        // Guard: null pointer (raw_ptr == 0) means null POINTER_TAG (0x7FFD_0000_0000_0000)
        // Produced by codegen bugs (uninitialized I64 NaN-boxed). Return undefined instead of crashing.
        if raw_ptr == 0 {
            eprintln!(
                "[NULL_PTR_METHOD_CALL] js_native_call_method: null pointer object for method '{}'",
                method_name
            );
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }

        // Buffer / Uint8Array dispatch — buffers are allocated raw without
        // a GcHeader, so the GC type check below would read random bytes
        // before the buffer storage and may accidentally match GC_TYPE_OBJECT.
        // Detect buffers via the BUFFER_REGISTRY first and route through the
        // dedicated dispatcher.
        if crate::buffer::is_registered_buffer(raw_ptr) {
            return Some(dispatch_buffer_method(
                raw_ptr,
                method_name,
                args_ptr,
                args_len,
            ));
        }

        // TypedArray method dispatch for NaN-boxed (POINTER_TAG) receivers.
        // The raw-pointer path above (#654) only fires when codegen leaves the
        // typed-array pointer untagged; a `Uint8Array` local loaded as a value
        // is NaN-boxed with POINTER_TAG and reaches here instead. Route the
        // callback-bearing + immutable methods to the shared helper before the
        // GC_TYPE_ARRAY check below (which only matches plain arrays).
        // Issues #2797 / #2798 / #2799.
        if crate::typedarray::lookup_typed_array_kind(raw_ptr).is_some() {
            let ta = raw_ptr as *mut crate::typedarray::TypedArrayHeader;
            if let Some(r) = dispatch_typed_array_method(ta, method_name, args_ptr, args_len) {
                return Some(r);
            }
        }

        // Builtin-prototype borrowing is lowered to a direct receiver call
        // (`[].slice.call(arguments, 1)` -> `arguments.slice(1)`). Arguments
        // objects do not expose Array methods as properties, but this dynamic
        // dispatch path preserves the borrowed Array.prototype.slice behavior.
        if method_name == "slice" {
            if let Some(args_arr) =
                crate::object::arguments_object_to_array(raw_ptr as *const ObjectHeader)
            {
                let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                let arg_value = |i: usize| -> f64 {
                    if i < args_len && !args_ptr.is_null() {
                        *args_ptr.add(i)
                    } else {
                        undefined
                    }
                };
                let result =
                    crate::array::js_array_slice_values(args_arr, arg_value(0), arg_value(1));
                return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
            }
        }

        // Array method dispatch: when the object is a real or lazy array at runtime,
        // dispatch callback-bearing array methods directly to the array runtime helpers.
        // This covers the `anyTypedVar.map(fn)` / `anyTypedVar.filter(fn)` pattern where
        // the HIR lowering conservatively skipped Expr::ArrayMap/Filter because the
        // receiver's static type was `any` and the method name overlaps with user-class
        // method names — see the `is_class_overlapping_method` guard in expr_call.rs
        // (issue #267). The GC type check here ensures we only intercept when the
        // value is actually an array; user-class instances with a `.map` closure field
        // fall through to the object-field scan below unchanged.
        if raw_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let arr_gc_hdr =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            let arr_obj_type = (*arr_gc_hdr).obj_type;
            if arr_obj_type == crate::gc::GC_TYPE_ARRAY
                || arr_obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
            {
                // A user-stored callable own property on the array
                // (`arr.getClass = Object.prototype.toString; arr.getClass()`,
                // `arr.myFn = function(){...}; arr.myFn()`) must win over the
                // built-in array method arms below. Array named properties live
                // in the ARRAY_NAMED_PROPS side table, NOT in `keys_array`, so
                // the generic own-field scan further down never finds them and
                // `arr.<name>()` wrongly fell through to a built-in (e.g.
                // `arr.toString()` shadowed by a stored `getClass` resolved as
                // the array's own toString). Check the side table first and, if
                // the stored value is callable, invoke it with `this` = arr.
                let arr = raw_ptr as *const crate::array::ArrayHeader;
                if let Some(stored) =
                    crate::array::array_named_property_get_by_name(arr, method_name)
                {
                    let stored_ptr = crate::value::js_nanbox_get_pointer(stored) as usize;
                    if crate::closure::is_closure_ptr(stored_ptr) {
                        let recv_bits = jsval.bits();
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                        let result =
                            crate::closure::js_native_call_value(stored, args_ptr, args_len);
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return Some(result);
                    }
                }
                match method_name {
                    "toString" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let s = crate::array::js_array_join_value(
                            arr,
                            f64::from_bits(crate::value::TAG_UNDEFINED),
                        );
                        return Some(f64::from_bits(JSValue::string_ptr(s).bits()));
                    }
                    "map" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr =
                            crate::array::js_validate_array_map_callback(arr as i64, *args_ptr)
                                as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_map(arr, cb_ptr);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "filter" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_filter(arr, cb_ptr);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    // Issue #493 followup: dispatch `forEach` on any-typed
                    // arrays the same way as map/filter. Codegen's HIR-level
                    // `Expr::ArrayForEach` only fires for receivers it can
                    // statically prove are arrays — rest params and other
                    // dynamically-typed receivers fall through to the runtime
                    // dispatch tower, where this arm now intercepts. Without
                    // it, `args.forEach(cb)` (where `args` is a closure rest
                    // param threaded across module boundaries) silently
                    // no-op'd, breaking hono's route-registration loop and
                    // any other code that does the same arrow-rest-forEach
                    // pattern.
                    "forEach" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        crate::array::js_array_forEach(arr, cb_ptr);
                        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
                    }
                    // Issue #291: defensive `slice` arm for arrays that
                    // reach the generic dispatch tower (e.g. when the
                    // receiver is `Expr::Logical` / `Expr::Conditional` /
                    // `any`-typed `Expr::Call` and codegen's
                    // `is_array_expr` returned false). Without this arm
                    // the fallthrough returned the static `NULL_OBJECT_BYTES`
                    // sentinel and the next chained operation segfaulted.
                    "slice" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                        let arg_value = |i: usize| -> f64 {
                            if i < args_len && !args_ptr.is_null() {
                                *args_ptr.add(i)
                            } else {
                                undefined
                            }
                        };
                        let result = if let Some(args_arr) =
                            crate::object::arguments_object_to_array(
                                raw_ptr as *const crate::object::ObjectHeader,
                            ) {
                            crate::array::js_array_slice_values(
                                args_arr,
                                arg_value(0),
                                arg_value(1),
                            )
                        } else {
                            crate::array::js_array_slice_values(arr, arg_value(0), arg_value(1))
                        };
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    // Issue #321 (effect Context/Layer): defensive `splice`
                    // arm for any-typed arrays that reach the generic dispatch
                    // tower. The sibling `slice`/`sort`/`reverse` arms exist
                    // but `splice` was missing, so effect's FiberRuntime op
                    // queue (`(arr as any).splice(start, deleteCount)`) threw
                    // "splice is not a function". Mirrors JS semantics:
                    // mutates the receiver in place and returns a new array of
                    // the removed elements. Extra args after deleteCount are
                    // inserted at `start`.
                    "splice" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        // ToIntegerOrInfinity with i32 clamping: NaN → 0,
                        // +Infinity → i32::MAX (clamps to len downstream),
                        // -Infinity → i32::MIN (relative-from-end → 0). The
                        // old `is_infinite() → 0` made `splice(Infinity, 3)`
                        // delete from the front (test262 S15.4.4.12_A2.1_T3).
                        let arg_i32 = |i: usize| -> i32 {
                            if i < args_len && !args_ptr.is_null() {
                                crate::array::js_array_splice_delete_count(*args_ptr.add(i))
                            } else {
                                0
                            }
                        };
                        let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                        // Per spec: splice() deletes nothing, while
                        // splice(start) deletes through the end.
                        let delete_count = if args_len == 0 {
                            0
                        } else if args_len == 1 {
                            i32::MAX
                        } else {
                            arg_i32(1)
                        };
                        // Items to insert are args[2..].
                        let items: Vec<f64> = if args_len > 2 && !args_ptr.is_null() {
                            std::slice::from_raw_parts(args_ptr.add(2), args_len - 2).to_vec()
                        } else {
                            Vec::new()
                        };
                        let items_ptr = if items.is_empty() {
                            std::ptr::null()
                        } else {
                            items.as_ptr()
                        };
                        let mut out_arr: *mut crate::array::ArrayHeader = std::ptr::null_mut();
                        let deleted = crate::array::js_array_splice(
                            arr,
                            start,
                            delete_count,
                            items_ptr,
                            items.len() as u32,
                            &mut out_arr,
                        );
                        return Some(f64::from_bits(JSValue::pointer(deleted as *mut u8).bits()));
                    }
                    "pop" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        return Some(crate::array::js_array_pop_f64(arr));
                    }
                    "push" => {
                        // Spec §23.1.3.21: Set(O,"length",…) fires even with 0
                        // args, so frozen / non-writable-length must throw.
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        if crate::array::array_is_frozen(arr) {
                            crate::collection_iter::throw_type_error(
                                "Cannot mutate a frozen array",
                            );
                        }
                        crate::array::guard_writable_length(arr);
                        let mut a = arr;
                        for i in 0..args_len {
                            let v = if !args_ptr.is_null() {
                                unsafe { *args_ptr.add(i) }
                            } else {
                                f64::from_bits(crate::value::TAG_UNDEFINED)
                            };
                            a = crate::array::js_array_push_f64(a, v);
                        }
                        return Some(crate::array::js_array_length(a) as f64);
                    }
                    "shift" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        return Some(crate::array::js_array_shift_f64(arr));
                    }
                    "unshift" => {
                        // #2814: zero-arg returns current length (no mutation);
                        // 1+ args insert all items at the front in source order.
                        // Route the zero-arg case through `js_array_unshift_variadic`
                        // (count 0) as well, so a non-writable `length` still throws
                        // the spec TypeError (`Set(O,"length",…)` always runs).
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let count = if args_ptr.is_null() {
                            0
                        } else {
                            args_len as u32
                        };
                        let result = crate::array::js_array_unshift_variadic(arr, args_ptr, count);
                        return Some(crate::array::js_array_length(result) as f64);
                    }
                    // Issue #515 followup: defensive `with` arm for arrays that
                    // reach the generic dispatch tower because the HIR fold
                    // bailed (untyped receiver, chained call returning Array,
                    // etc.). Without this arm, tightening the HIR fold to
                    // ignore unknown-type receivers would silently break
                    // legitimate `(arr: any).with(idx, val)` callers.
                    "with" if args_len >= 2 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let index = *args_ptr;
                        let value = *args_ptr.add(1);
                        let result = crate::array::js_array_with(arr, index, value);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    // Issue #546 followup: defensive `some` / `every` /
                    // `find` / `findIndex` / `findLast` / `findLastIndex`
                    // arms for any-typed receivers that escape the HIR
                    // fast-path. The `is_class_overlapping_method` guard
                    // (expr_call.rs ~2621) bails on Any-typed locals — so
                    // a destructured `const { arr } = entry; arr.some(cb)`
                    // (where `arr` lost its `EntityId<any>[]` type through
                    // destructuring) silently fell through to the object
                    // field-scan and returned the array itself, producing
                    // `typeof = object` instead of a boolean. The hooks
                    // module in @codehz/ecs hits this exact pattern in
                    // `triggerMultiComponentHooks`, so on_set never fired.
                    "some" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return Some(crate::array::js_array_some(arr, cb_ptr));
                    }
                    "every" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return Some(crate::array::js_array_every(arr, cb_ptr));
                    }
                    "find" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return Some(crate::array::js_array_find(arr, cb_ptr));
                    }
                    "findIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_findIndex(arr, cb_ptr);
                        return Some(idx as f64);
                    }
                    "findLast" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        return Some(crate::array::js_array_find_last(arr, cb_ptr));
                    }
                    "findLastIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_find_last_index(arr, cb_ptr);
                        return Some(idx as f64);
                    }
                    // Issue #587: `str.split(sep).map(fn).sort()` returned ""
                    // because chained `.sort()` falls through HIR's array-fold
                    // (the `"sort" if !args.is_empty()` arm in expr_call.rs
                    // requires a comparator) and lands here. Without these
                    // arms the very-end fallthrough returns NULL_OBJECT_BYTES,
                    // which JSON.stringify renders as "". The s3-lite-client
                    // SigV4 canonical-query-string builder
                    // (`.split("&").map(...).sort().join("&")`) was the
                    // load-bearing user impact. Same gap for `.reverse()` —
                    // tracked by issue #587's regressions list. Adding
                    // `reduce` / `reduceRight` / `flat` / `flatMap` / `concat`
                    // / `indexOf` / `includes` / `at` / `fill` while we're
                    // here defensively, since they have the same shape and
                    // share the HIR-fold escape risk for chained-call
                    // receivers.
                    "sort" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        // #2796: validate comparator (function | undefined) before sorting.
                        let result = if args_len >= 1 && !args_ptr.is_null() {
                            let cb_ptr = crate::array::js_validate_array_comparator(*args_ptr)
                                as *const crate::closure::ClosureHeader;
                            crate::array::js_array_sort_with_comparator(arr, cb_ptr)
                        } else {
                            crate::array::js_array_sort_default(arr)
                        };
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "reverse" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_reverse(arr);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "reduce" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return Some(crate::array::js_array_reduce(arr, cb_ptr, has_init, init));
                    }
                    "reduceRight" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return Some(crate::array::js_array_reduce_right(
                            arr, cb_ptr, has_init, init,
                        ));
                    }
                    "flat" => {
                        // #2800: honor the optional depth argument. Omitted →
                        // depth 1 (legacy `js_array_flat`); supplied → route to
                        // the depth-aware helper, which applies JS number
                        // coercion (NaN/≤0 → 0, +Infinity → fully flat).
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result = if args_len >= 1 && !args_ptr.is_null() {
                            crate::array::js_array_flat_depth(arr, *args_ptr)
                        } else {
                            crate::array::js_array_flat(arr)
                        };
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "flatMap" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #4091: throw TypeError for a non-callable callback.
                        let cb_ptr = crate::array::js_validate_array_callback(*args_ptr)
                            as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_flatMap(arr, cb_ptr);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "concat" => {
                        // #2805: non-mutating, variadic concat with
                        // Symbol.isConcatSpreadable handling.
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result =
                            crate::array::js_array_concat_variadic(arr, args_ptr, args_len as i32);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "indexOf" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2804: honor the optional fromIndex (2nd arg).
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let (from_index, has_from) = if args_len >= 2 {
                            (*args_ptr.add(1), 1)
                        } else {
                            (0.0, 0)
                        };
                        return Some(crate::array::js_array_indexOf_jsvalue(
                            arr, value, from_index, has_from,
                        ) as f64);
                    }
                    "includes" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2804: honor the optional fromIndex (2nd arg).
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let (from_index, has_from) = if args_len >= 2 {
                            (*args_ptr.add(1), 1)
                        } else {
                            (0.0, 0)
                        };
                        let r = crate::array::js_array_includes_jsvalue(
                            arr, value, from_index, has_from,
                        );
                        return Some(f64::from_bits(JSValue::bool(r != 0).bits()));
                    }
                    "lastIndexOf" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        // Optional fromIndex (2nd arg); absent → has_from=0.
                        let (from_index, has_from) = if args_len >= 2 {
                            (*args_ptr.add(1), 1)
                        } else {
                            (0.0, 0)
                        };
                        return Some(crate::array::js_array_last_index_of_jsvalue(
                            arr, value, from_index, has_from,
                        ) as f64);
                    }
                    "at" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        return Some(crate::array::js_array_at(arr, *args_ptr));
                    }
                    "fill" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2801: honor the optional start/end range. One arg →
                        // whole-array fill; 2+ args → range fill with the
                        // supplied start and (defaulting to +Infinity →
                        // clamps to length) end, mirroring the static path.
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let result = if args_len >= 2 {
                            let start = *args_ptr.add(1);
                            let end = if args_len >= 3 {
                                *args_ptr.add(2)
                            } else {
                                f64::INFINITY
                            };
                            crate::array::js_array_fill_range(arr, value, start, end)
                        } else {
                            crate::array::js_array_fill(arr, value)
                        };
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "copyWithin" if args_len >= 1 && !args_ptr.is_null() => {
                        // #2802: dynamic dispatch for Array.prototype.copyWithin.
                        // Mirrors the static codegen path: require `target`,
                        // default omitted `start` to 0, pass has_end=0 when
                        // `end` is omitted. Mutates and returns the receiver.
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let target = *args_ptr;
                        let start = if args_len >= 2 { *args_ptr.add(1) } else { 0.0 };
                        let (has_end, end) = if args_len >= 3 {
                            (1, *args_ptr.add(2))
                        } else {
                            (0, 0.0)
                        };
                        let result =
                            crate::array::js_array_copy_within(arr, target, start, has_end, end);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "join" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let separator = if args_len >= 1 && !args_ptr.is_null() {
                            *args_ptr
                        } else {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        };
                        let s = crate::array::js_array_join_value(arr, separator);
                        return Some(f64::from_bits(JSValue::string_ptr(s).bits()));
                    }
                    // #321: a value-level `arr[Symbol.iterator]()` resolves to
                    // the array's bound `values` method (see symbol.rs), and
                    // `arr.values()`/`.keys()`/`.entries()` reaching the runtime
                    // dispatch tower (not codegen's eager `Expr::ArrayValues`
                    // fast path) must return a real `.next()`-bearing iterator,
                    // not an eager array clone. Effect's `Chunk[Symbol.iterator]`
                    // delegates to `backing.array[Symbol.iterator]()` and then
                    // `Array.from`/`Arr.reduce` drive `.next()` on the result;
                    // without this the call returned `undefined` and surfaced as
                    // `Cannot read properties of undefined (reading '_tag')`.
                    "values" | "Symbol.iterator" | "@@iterator" => {
                        return Some(crate::array::array_values_iter(object));
                    }
                    "keys" => {
                        return Some(crate::array::array_keys_iter(object));
                    }
                    "entries" => {
                        return Some(crate::array::array_entries_iter(object));
                    }
                    // #2803: ES2023 immutable methods reaching the dynamic
                    // dispatch tower (`(arr as any).toSorted()`, computed
                    // `arr[m]()`, chained-call receivers that escape the HIR
                    // fold). Each returns a NEW array and leaves the receiver
                    // unchanged, mirroring the static codegen helpers.
                    "toReversed" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result = crate::array::js_array_to_reversed(arr);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "toSorted" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // #2796: validate comparator (function | undefined);
                        // a null/undefined comparator routes to the default
                        // (string) sort inside js_array_to_sorted_with_comparator.
                        let cmp_ptr = if args_len >= 1 && !args_ptr.is_null() {
                            crate::array::js_validate_array_comparator(*args_ptr)
                                as *const crate::closure::ClosureHeader
                        } else {
                            std::ptr::null()
                        };
                        let result = crate::array::js_array_to_sorted_with_comparator(arr, cmp_ptr);
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    "toSpliced" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        // Per spec / #2794: toSpliced() inserts/deletes nothing,
                        // toSpliced(start) deletes through the end. NaN-coercion
                        // for the f64 start/deleteCount is handled in the helper.
                        let start = if args_len >= 1 { *args_ptr } else { 0.0 };
                        let delete_count = if args_len == 0 {
                            0.0
                        } else if args_len == 1 {
                            f64::INFINITY
                        } else {
                            *args_ptr.add(1)
                        };
                        let items: Vec<f64> = if args_len > 2 && !args_ptr.is_null() {
                            std::slice::from_raw_parts(args_ptr.add(2), args_len - 2).to_vec()
                        } else {
                            Vec::new()
                        };
                        let items_ptr = if items.is_empty() {
                            std::ptr::null()
                        } else {
                            items.as_ptr()
                        };
                        let result = crate::array::js_array_to_spliced(
                            arr,
                            start,
                            delete_count,
                            items_ptr,
                            items.len() as u32,
                        );
                        return Some(f64::from_bits(JSValue::pointer(result as *mut u8).bits()));
                    }
                    // #2808: Array.prototype.toLocaleString — calls each
                    // non-nullish element's own toLocaleString(locales, options),
                    // renders nullish/hole elements as empty fields, and joins
                    // with commas. Routed here for any-typed / computed receivers.
                    "toLocaleString" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let locales = if args_len >= 1 && !args_ptr.is_null() {
                            *args_ptr
                        } else {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        };
                        let options = if args_len >= 2 && !args_ptr.is_null() {
                            *args_ptr.add(1)
                        } else {
                            f64::from_bits(crate::value::TAG_UNDEFINED)
                        };
                        let s = crate::array::js_array_to_locale_string(arr, locales, options);
                        return Some(f64::from_bits(JSValue::string_ptr(s).bits()));
                    }
                    _ => {} // not a handled array method — fall through to object dispatch
                }
            }
        }

        // Check if this is a native module namespace object (e.g., fs, os, path)
        let obj = jsval.as_pointer::<ObjectHeader>();
        // Validate GcHeader to confirm this is actually an object before reading class_id
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
            if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
                // #853: the `is_valid_obj_ptr` guard that used to live after
                // this return was dead — the early return claims the path
                // unconditionally. Removed.
                return Some(
                    crate::object::native_module::call_native_module_dispatch_hook(
                        obj,
                        method_name,
                        args_ptr,
                        args_len,
                    ),
                );
            }
            // Issue #1206: Buffer iterators returned from `buf.values()` etc.
            // have a dedicated class id so `.next()` lands here and dispatches
            // to the iterator-protocol helper without paying the generic
            // closure-field scan below.
            if (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID {
                return Some(crate::buffer::dispatch_buffer_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            // #321: array iterators returned from a value-level
            // `arr.values()`/`.keys()`/`.entries()`/`[Symbol.iterator]()`
            // carry a dedicated class id so `.next()` lands in the iterator
            // dispatcher (matching the Buffer iterator above).
            if (*obj).class_id == crate::array::ARRAY_ITERATOR_CLASS_ID {
                return Some(crate::array::dispatch_array_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                ));
            }
            if let Some(result) =
                crate::node_test::dispatch_object_method((*obj).class_id, method_name)
            {
                return Some(result);
            }
            // #2856: Map/Set iterators returned from a value-level
            // `m.entries()`/`.keys()`/`.values()` / `s.entries()` etc. carry
            // dedicated class ids so `.next()` lands in the matching iterator
            // dispatcher (mirroring the array iterator above).
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
            // #2874: lazy iterator-helper objects (`Iterator.from(x)` and the
            // chain it produces: `.map`/`.filter`/`.take`/`.drop`/`.flatMap`/
            // `.toArray`/`.forEach`/`.reduce`/`.some`/`.every`/`.find`/`.next`).
            if (*obj).class_id == crate::iterator_helpers::ITERATOR_HELPER_CLASS_ID {
                return Some(crate::iterator_helpers::dispatch_iterator_helper_method(
                    obj as *mut ObjectHeader,
                    method_name,
                    args_ptr,
                    args_len,
                ));
            }

            // #2874: an iterator-helper method (`map`/`filter`/`take`/…) on a
            // RAW iterator object — a generator, the runtime array/Map/Set
            // iterators, or any `{ next() }`. Node resolves these on
            // `Iterator.prototype`; wrap the iterator in an identity helper and
            // dispatch there. Skipped when the object defines the name as an own
            // callable field (the user's own method wins). Runs before the
            // own-field scan so the cheap has-own check below stays in sync.
            if crate::iterator_helpers::is_iterator_helper_method(method_name) {
                let has_own = {
                    let mk = crate::string::js_string_from_bytes(
                        method_name.as_ptr(),
                        method_name.len() as u32,
                    );
                    let fv = js_object_get_field_by_name(obj as *const _, mk);
                    let fp =
                        crate::value::js_nanbox_get_pointer(f64::from_bits(fv.bits())) as usize;
                    !fv.is_undefined() && crate::closure::is_closure_ptr(fp)
                };
                if let Some(result) = crate::iterator_helpers::maybe_dispatch_helper_on_iterator(
                    obj as *mut ObjectHeader,
                    method_name,
                    args_ptr,
                    args_len,
                    has_own,
                ) {
                    return Some(result);
                }
            }

            // Scan object fields for a callable property (closure stored via IndexSet)
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
                                // Always try the field as a callable —
                                // `js_native_call_value` validates
                                // CLOSURE_MAGIC internally and safely
                                // returns undefined for non-callables.
                                // The previous `is_pointer()` gate bailed
                                // on raw-pointer-bit values (e.g. the
                                // Promise executor's resolve/reject
                                // closures — stored as
                                // `transmute(ptr → f64)` without a
                                // POINTER_TAG). That turned
                                // `box.resolve(val)` into a no-op that
                                // returned the raw pointer bits instead
                                // of invoking `js_promise_resolve`, so
                                // the outer `await` hung forever
                                // (issue #87).
                                //
                                // Issue #519: bind `this` to the receiver
                                // for the duration of the call. Non-arrow
                                // function bodies read `this` from
                                // IMPLICIT_THIS (codegen Expr::This
                                // fallback when this_stack is empty);
                                // without this save/set/restore, the
                                // body sees `this = undefined` and any
                                // `this.foo()` call falls through to the
                                // issue #510 catch-all "(undefined).foo
                                // is not a function" TypeError. Hono's
                                // RegExpRouter.match (imported function
                                // assigned as a class field) hit this.
                                let recv_bits = jsval.bits();
                                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                                let result = crate::closure::js_native_call_value(
                                    f64::from_bits(field_val.bits()),
                                    args_ptr,
                                    args_len,
                                );
                                IMPLICIT_THIS.with(|c| c.set(prev_this));
                                return Some(result);
                            }
                        }
                    }
                }
            }

            // Vtable lookup for class instances — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = jsval.as_pointer::<u8>() as i64;
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
                // Refs #420: walk the parent chain via the class registry. Per
                // JS spec, `subInstance.method()` for a method defined on a
                // parent dispatches to the parent's implementation — drizzle's
                // `serial("id").primaryKey()` where primaryKey is on
                // ColumnBuilder (grandparent) but the receiver is a
                // PgSerialBuilder (grandchild). The codegen-side dispatch tower
                // in `lower_call.rs` only registers classes the importing module
                // knows about; for not-by-name-imported subclasses (return
                // values of imported functions) we depend on this runtime walk.
                //
                // DEADLOCK SAFETY: resolve the target under the registry READ
                // lock, then DROP the lock before invoking the method body.
                // A user method body can lazily init a module (function-local
                // `require()` — Next.js `getServerImpl()` → `require('./next-
                // server')`) whose top-level `class` declarations call
                // `js_register_class_method` → a registry WRITE lock. std
                // `RwLock` is not re-entrant, so holding the read guard across
                // the call deadlocked the (single) main thread.
                enum ResolvedMethod {
                    Vtable {
                        func_ptr: usize,
                        param_count: u32,
                        has_synthetic_arguments: bool,
                        has_rest: bool,
                        this_i64: i64,
                    },
                    // #711 part 2 / #321: a method that is an own-property of a
                    // registered prototype object (`Function.prototype = X`,
                    // effect's `EffectPrototype.pipe`).
                    ProtoClosure {
                        field_bits: u64,
                    },
                }
                let mut resolved_method: Option<ResolvedMethod> = None;
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
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
                                    resolved_method = Some(ResolvedMethod::Vtable {
                                        func_ptr: entry.func_ptr,
                                        param_count: entry.param_count,
                                        has_synthetic_arguments: entry.has_synthetic_arguments,
                                        has_rest: entry.has_rest,
                                        this_i64: jsval.as_pointer::<u8>() as i64,
                                    });
                                    break;
                                }
                            }
                            let proto_obj = class_prototype_object(cur_cid);
                            if !proto_obj.is_null() {
                                let method_key = crate::string::js_string_from_bytes(
                                    method_name.as_ptr(),
                                    method_name.len() as u32,
                                );
                                // An inherited method that resolves to an ACCESSOR
                                // on the prototype must observe the instance as
                                // `this` (spec `[[Get]](P, Receiver)`), not the
                                // prototype object the getter lives on. Stash the
                                // receiver so `invoke_accessor_getter` rebinds it.
                                // Without this, a schema library's lazily-installed
                                // `describe`/`clone` accessors — `Object.define-
                                // Property(proto, k, { get() { const b = fn.bind(this);
                                // Object.defineProperty(this, k, { value: b }); return b } })`
                                // on the shared class prototype — run with
                                // `this === prototype`, bake `this = prototype` into
                                // the returned bound method, and cache it on the
                                // prototype. Every downstream read of an
                                // instance-only field via `this.<field>` then
                                // returns `undefined` (`Cannot read properties of
                                // undefined`) even though the instance has it.
                                // Mirrors `resolve_proto_chain_field_with_receiver`
                                // (the winston `get transports()` fix).
                                let receiver_f64 = f64::from_bits(jsval.bits());
                                let prev_this =
                                    IMPLICIT_THIS.with(|c| c.replace(receiver_f64.to_bits()));
                                let prev_override =
                                    super::super::field_get_set::accessor_receiver_override_begin(
                                        receiver_f64,
                                    );
                                let field_val = js_object_get_field_by_name(
                                    proto_obj as *const _,
                                    method_key as *const crate::StringHeader,
                                );
                                super::super::field_get_set::accessor_receiver_override_end(
                                    prev_override,
                                );
                                IMPLICIT_THIS.with(|c| c.set(prev_this));
                                if !field_val.is_undefined() && !field_val.is_null() {
                                    resolved_method = Some(ResolvedMethod::ProtoClosure {
                                        field_bits: field_val.bits(),
                                    });
                                    break;
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
                // Registry guard released — safe to run the method body (which
                // may register classes via lazy module init).
                match resolved_method {
                    Some(ResolvedMethod::Vtable {
                        func_ptr,
                        param_count,
                        has_synthetic_arguments,
                        has_rest,
                        this_i64,
                    }) => {
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
                    Some(ResolvedMethod::ProtoClosure { field_bits }) => {
                        // #321 (effect Context/Layer/Scope): rebind the closure's
                        // `this` slot to the receiver — `clone_closure_rebind_this`
                        // is a no-op for closures that don't capture `this` and for
                        // non-closure values, so those paths are unaffected.
                        let bound = crate::closure::clone_closure_rebind_this(
                            field_bits,
                            f64::from_bits(jsval.bits()),
                        );
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(bound),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return Some(result);
                    }
                    None => {}
                }
                // #809: independent prototype-object resolution. The walk
                // above only runs when `CLASS_VTABLE_REGISTRY` is `Some` —
                // a program with no user classes that only does
                // `Object.create(objLiteral).method()` has an empty/None
                // registry, so `inst.method()` never reached
                // `class_prototype_object` and threw `<m> is not a
                // function`. Resolve the method off the synthetic-class-id
                // prototype chain directly (reuses the same helper as
                // `js_object_get_field_by_name`), then invoke it with
                // `this` bound to the receiver.
                let method_key = crate::string::js_string_from_bytes(
                    method_name.as_ptr(),
                    method_name.len() as u32,
                );
                // `_with_receiver` binds an inherited ACCESSOR getter's `this`
                // to the instance (not the prototype it lives on), matching the
                // ProtoClosure walk above and spec `[[Get]](P, Receiver)`. A
                // prototype `Object.defineProperty(proto, k, { get })` reached
                // through the registry-`None` (`Object.create(objLiteral)`) path
                // would otherwise observe the prototype as `this`.
                if let Some(field_val) = resolve_proto_chain_field_with_receiver(
                    class_id,
                    method_key as *const crate::StringHeader,
                    f64::from_bits(jsval.bits()),
                ) {
                    if !field_val.is_undefined() && !field_val.is_null() {
                        // #321 (effect Context/Layer/Scope): the closure we
                        // just resolved is an *inherited* method — by
                        // construction `resolve_proto_chain_field` only walks
                        // the prototype chain (the receiver's OWN fields are
                        // handled by the earlier keys-array scan), so this is
                        // never an own method. Object-literal methods are
                        // lowered with `captures_this:true` and have their
                        // reserved (last) capture slot patched to the literal
                        // object — i.e. the PROTOTYPE — at construction time
                        // (see `expr.rs::lower_object_literal` /
                        // `symbol.rs::js_object_set_symbol_method`). So when
                        // `o = Object.create(P)` resolves `o.method()`, the
                        // closure carries `this === P`, not `this === o`, and
                        // setting `IMPLICIT_THIS = o` can't override the
                        // baked-in slot that the body reads. Rebind the slot
                        // to the receiver before invoking. This mirrors the
                        // symbol-keyed fix (#1969) for the string-keyed
                        // static-member call path. `clone_closure_rebind_this`
                        // is a no-op for non-`captures_this` closures and for
                        // non-closure values, so inherited *data* properties
                        // and arrow/`this`-free function values are untouched.
                        let bound = crate::closure::clone_closure_rebind_this(
                            field_val.bits(),
                            f64::from_bits(jsval.bits()),
                        );
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(bound),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return Some(result);
                    }
                }

                // Issue #838: JS-classic `Class.prototype.method = fn`
                // method dispatch. The vtable / proto-object walks above
                // cover ES-class methods and synthetic-prototype-object
                // shapes; this arm catches the case where the method
                // only exists in `CLASS_PROTOTYPE_METHODS`. Bind `this`
                // to the receiver and call the stored closure.
                if let Some(method_value) = lookup_prototype_method(class_id, method_name) {
                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                    let result =
                        crate::closure::js_native_call_value(method_value, args_ptr, args_len);
                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                    return Some(result);
                }
            }
        }
    }

    None
}
