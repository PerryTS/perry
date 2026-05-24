//! `js_native_call_method` — the runtime dispatch tower for
//! dynamic method calls on any-typed receivers. Also the apply/spread
//! and computed-key variants (`js_native_call_method_apply`,
//! `js_native_call_method_str_key`).
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

/// Call a method on an object with dynamic dispatch
/// This is used for runtime method calls when the method cannot be resolved statically.
/// object: NaN-boxed f64 containing an object pointer
/// method_name_ptr: pointer to the method name string (raw bytes, not StringHeader)
/// method_name_len: length of the method name
/// args_ptr: pointer to array of f64 arguments
/// args_len: number of arguments
/// Returns the result as f64
///
/// NOTE: This function is named js_native_call_method to avoid symbol collision
/// with js_call_method in perry-jsruntime which handles V8 JavaScript values.

/// Apply form for method calls with spread arguments on dynamically-typed
/// receivers (refs #421). Reads `args_array_handle` (a JS array containing
/// v0.5.754: dispatch `obj[strKey](args)` — computed-key method call.
/// `name_handle` is a StringHeader pointer (already-unboxed). Extracts
/// the bytes/length from the header and forwards to
/// `js_native_call_method`. Refs #420 / drizzle's
/// `this.session[isOneTimeQuery ? "prepareOneTimeQuery" :
/// "prepareQuery"](...)` chain.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_str_key(
    object: f64,
    name_handle: i64,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if name_handle == 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let str_ptr = name_handle as *const crate::StringHeader;
    let bytes_ptr = (str_ptr as *const i8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes_len = (*str_ptr).byte_len as usize;
    js_native_call_method(object, bytes_ptr, bytes_len, args_ptr, args_len)
}

/// every regular + spread arg already concatenated by codegen), materialises
/// the f64 elements into a temporary `Vec<f64>`, and forwards to
/// `js_native_call_method`. Lets the caller use a single uniform shape for
/// `recv.method(...args)` without exposing array layout to the dispatcher.
#[no_mangle]
pub unsafe extern "C" fn js_native_call_method_apply(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_array_handle: i64,
) -> f64 {
    let arr = args_array_handle as *const crate::array::ArrayHeader;
    let len = if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr) as usize
    };
    let buf: Vec<f64> = (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i as u32))
        .collect();
    let (args_ptr, args_len) = if buf.is_empty() {
        (std::ptr::null::<f64>(), 0_usize)
    } else {
        (buf.as_ptr(), buf.len())
    };
    js_native_call_method(object, method_name_ptr, method_name_len, args_ptr, args_len)
}

#[inline]
fn root_string_arg_handle<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    arg_handles: &[crate::gc::RuntimeHandle<'scope>],
    index: usize,
) -> Option<crate::gc::RuntimeHandle<'scope>> {
    let value = arg_handles.get(index)?.get_nanbox_f64();
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() {
        None
    } else {
        Some(scope.root_string_ptr(ptr))
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_native_call_method(
    object: f64,
    method_name_ptr: *const i8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Get the method name (parsed early for depth guard logging)
    let method_name_owned = if method_name_ptr.is_null() || method_name_len == 0 {
        String::new()
    } else {
        let bytes = std::slice::from_raw_parts(method_name_ptr as *const u8, method_name_len);
        String::from_utf8_lossy(bytes).into_owned()
    };
    let method_name = method_name_owned.as_str();
    let root_scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = root_scope.root_nanbox_f64(object);
    let original_args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = root_scope.root_nanbox_f64_slice(&original_args);
    let refreshed_args = || crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    let object = object_handle.get_nanbox_f64();
    // RAII recursion depth guard: prevent stack overflow from circular module deps.
    // The guard auto-decrements on drop, covering all ~20 return points in this function.
    // When max depth is hit, return a pointer to a static empty object instead of undefined.
    // This prevents crashes when callers NaN-unbox the result and dereference it as a pointer.
    let _depth_guard = match CallMethodDepthGuard::enter(method_name) {
        Some(g) => g,
        None => {
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }
    };

    // Check if this is a JS handle (V8 object from JS runtime)
    if crate::value::is_js_handle(object) {
        let func_ptr =
            crate::value::JS_HANDLE_CALL_METHOD.load(std::sync::atomic::Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: unsafe extern "C" fn(f64, *const i8, usize, *const f64, usize) -> f64 =
                std::mem::transmute(func_ptr);
            let result = func(object, method_name_ptr, method_name_len, args_ptr, args_len);
            return result;
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    let jsval = JSValue::from_bits(object.to_bits());

    // Issue #489 followup: Promise's `then` / `catch` / `finally` are
    // intrinsic — when the dynamic dispatch path lands a `.then(cb)` on
    // a Promise (drizzle's `mysql-proxy/session.js`:
    // `this.client(...).then(({rows}) => rows)` where the static
    // analyzer couldn't prove the receiver is a Promise), route directly
    // to `js_promise_then` / `js_promise_catch` / `js_promise_finally`.
    // Without this, the field-scan + class-id walks below find nothing
    // and return undefined — drizzle's `MySqlRemoteSession.all` then
    // resolves to undefined and downstream `data[0].insertId` accesses
    // silently fail.
    if matches!(method_name, "then" | "catch" | "finally")
        && crate::promise::js_value_is_promise(object_handle.get_nanbox_f64()) != 0
    {
        let promise_ptr = (object_handle.get_nanbox_f64().to_bits() & 0x0000_FFFF_FFFF_FFFF)
            as *mut crate::Promise;
        let promise_handle = root_scope.root_raw_mut_ptr(promise_ptr);
        let args = refreshed_args();
        let arg0_box = if !args.is_empty() {
            args[0]
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let arg1_box = if args.len() >= 2 {
            args[1]
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        // Closures arrive here in two shapes:
        //  - NaN-boxed `POINTER_TAG | (closure_ptr & 0x0000_FFFF_FFFF_FFFF)`
        //    (the codegen `js_closure_alloc_singleton` + OR-with-tag form)
        //  - Raw `*ClosureHeader` bit-cast to f64 — the convention used
        //    by `js_assimilate_thenable` when it propagates
        //    `then(resolve, reject)` callbacks through a user-defined
        //    `then` method's param slots (see `promise.rs:2438-2442`).
        // Accept both. TAG_UNDEFINED / null / non-pointer values stay
        // null so `js_promise_then` treats the handler as missing.
        let extract_closure = |v: f64| -> crate::promise::ClosurePtr {
            let b = v.to_bits();
            let candidate = if (b & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
                b & 0x0000_FFFF_FFFF_FFFF
            } else if (b & 0xFFFF_0000_0000_0000) == 0 {
                b
            } else {
                0
            };
            if candidate < 0x10000 {
                std::ptr::null()
            } else {
                candidate as crate::promise::ClosurePtr
            }
        };
        let result = match method_name {
            "then" => crate::promise::js_promise_then(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
                extract_closure(arg1_box),
            ),
            "catch" => crate::promise::js_promise_catch(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
            ),
            "finally" => crate::promise::js_promise_finally(
                promise_handle.get_raw_mut_ptr(),
                extract_closure(arg0_box),
            ),
            _ => unreachable!(),
        };
        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
    }

    // Node timer handles are represented in Perry as small integer ids
    // NaN-boxed as pointers. Provide the common Timeout/Immediate methods
    // directly so `timeout.ref().unref().hasRef()` style probes behave like
    // Node without having to allocate a full JS wrapper object per timer.
    //
    // Gated on (a) tag == POINTER_TAG (0x7FFD) to avoid catching strings /
    // int32 / nullish tags, and (b) the id being a known timer so unrelated
    // small handles (UI widgets, drizzle, native instances) fall through
    // to the normal dispatch.
    {
        let bits = object.to_bits();
        let top16 = bits >> 48;
        if top16 == 0x7FFD {
            let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
            if crate::timer::is_known_timer_id(id) {
                match method_name {
                    "ref" => {
                        crate::timer::js_timer_ref(id);
                        return object;
                    }
                    "unref" => {
                        crate::timer::js_timer_unref(id);
                        return object;
                    }
                    "hasRef" => {
                        return if crate::timer::js_timer_has_ref(id) != 0 {
                            f64::from_bits(JSValue::bool(true).bits())
                        } else {
                            f64::from_bits(JSValue::bool(false).bits())
                        };
                    }
                    "refresh" => {
                        crate::timer::js_timer_refresh(id);
                        return object;
                    }
                    "close" => {
                        crate::timer::clearTimeout(id);
                        crate::timer::clearInterval(id);
                        return object;
                    }
                    // `__perry_dispose__` is the class-member form; the
                    // well-known `Symbol.dispose` computed form lowers to
                    // `@@__perry_wk_dispose`. Both clear the timer (#1213).
                    "__perry_dispose__" | "@@__perry_wk_dispose" => {
                        crate::timer::clearTimeout(id);
                        crate::timer::clearInterval(id);
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    "@@__perry_wk_toPrimitive" | "valueOf" => return id as f64,
                    _ => {}
                }
            }
        }
    }

    // Symbols: Symbol.for() pointers are Box-leaked (no GcHeader), so the
    // ObjectHeader path below would dereference garbage. Detect symbols
    // up front via the side-table.
    if jsval.is_pointer() {
        let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::symbol::is_registered_symbol(raw_ptr) {
            let sym_f64 = object;
            return match method_name {
                "toString" => {
                    let s = crate::symbol::js_symbol_to_string(sym_f64);
                    f64::from_bits(JSValue::string_ptr(s as *mut crate::StringHeader).bits())
                }
                "valueOf" => sym_f64,
                "description" => {
                    f64::from_bits(crate::symbol::js_symbol_description(sym_f64).to_bits())
                }
                _ => f64::from_bits(crate::value::TAG_UNDEFINED),
            };
        }
    }

    // Handle BigInt method calls (NaN-boxed with BIGINT_TAG 0x7FFA)
    if jsval.is_bigint() {
        let bigint_ptr = crate::bigint::clean_bigint_ptr(
            (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader,
        );
        match method_name {
            "isZero" => {
                let result = crate::bigint::js_bigint_is_zero(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "isNeg" | "isNegative" => {
                let result = crate::bigint::js_bigint_is_negative(bigint_ptr);
                return f64::from_bits(JSValue::bool(result != 0).bits());
            }
            "toNumber" => {
                return crate::bigint::js_bigint_to_f64(bigint_ptr);
            }
            "toString" => {
                let result_ptr = if args_len > 0 && !args_ptr.is_null() {
                    let radix_f64 = *args_ptr;
                    let radix = radix_f64 as i32;
                    crate::bigint::js_bigint_to_string_radix(bigint_ptr, radix)
                } else {
                    crate::bigint::js_bigint_to_string(bigint_ptr)
                };
                return f64::from_bits(JSValue::string_ptr(result_ptr).bits());
            }
            "add" | "sub" | "mul" | "div" | "mod" | "umod" | "pow" | "and" | "or" | "xor"
            | "shln" | "shrn" | "maskn" | "eq" | "lt" | "lte" | "gt" | "gte" | "cmp"
            | "fromTwos" | "toTwos" => {
                let args = refreshed_args();
                return dispatch_bigint_binary_method(
                    bigint_ptr,
                    method_name,
                    args.as_ptr(),
                    args.len(),
                );
            }
            _ => {
                // Unknown BigInt method - fall through to general dispatch
            }
        }
    }

    // Check for raw handle integer: Perry may bit-cast an i64 handle directly to f64,
    // producing a subnormal float (bits == handle_id, no NaN-box tag). Values 0 < bits < 0x100000
    // with no tag are raw handle IDs from Perry's integer-typed handle parameters.
    let raw_bits = object.to_bits();
    if raw_bits > 0 && raw_bits < 0x100000 {
        if let Some(dispatch) = handle_method_dispatch() {
            let args = refreshed_args();
            return dispatch(
                raw_bits as i64,
                method_name.as_ptr(),
                method_name.len(),
                args.as_ptr(),
                args.len(),
            );
        }
        return f64::from_bits(0x7FF8_0000_0000_0001); // undefined
    }

    // #1545: Web Streams handles are returned as `id as f64` (a normal float),
    // so their `to_bits()` is large and the raw-handle check above misses them.
    // When the receiver is a finite whole number and the stdlib probe confirms
    // it's a live stream handle, route the call through the same handle
    // dispatcher (which carries the stream method arms). Gating on the probe
    // means a genuine numeric receiver calling an unknown method still falls
    // through to the `(number).x is not a function` TypeError below.
    if object.is_finite() && object > 0.0 && object.fract() == 0.0 {
        let id = object as usize;
        if let Some(probe) = stream_handle_probe() {
            if probe(id) {
                if let Some(dispatch) = handle_method_dispatch() {
                    let args = refreshed_args();
                    return dispatch(
                        id as i64,
                        method_name.as_ptr(),
                        method_name.len(),
                        args.as_ptr(),
                        args.len(),
                    );
                }
            }
        }
    }

    // Issue #654: typed-array method dispatch. The codegen for
    // `new Float64Array(...)` (and the other typed-array constructors)
    // returns the raw heap pointer bitcast to f64 — no POINTER_TAG —
    // so neither `is_pointer()` nor the handle dispatch above catches
    // it. Detect via the `TYPED_ARRAY_REGISTRY` side table and route
    // common methods (`sort`, `at`, `toSorted`, `toReversed`, `with`,
    // `findLast`, `findLastIndex`) to their `js_typed_array_*` runtime
    // helpers. Without this arm `(a: Float64Array).sort()` reached the
    // `(number).sort is not a function` catch-all because raw pointer
    // bits classify as `is_number()` (top16 outside the tagged range).
    {
        let top16 = raw_bits >> 48;
        if top16 == 0 && raw_bits >= 0x10000 {
            let addr = raw_bits as usize;
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                let ta = addr as *mut crate::typedarray::TypedArrayHeader;
                let arg0 = || -> f64 {
                    if args_len >= 1 && !args_ptr.is_null() {
                        unsafe { *args_ptr }
                    } else {
                        f64::NAN
                    }
                };
                let arg_closure = |i: usize| -> *const crate::closure::ClosureHeader {
                    if i < args_len && !args_ptr.is_null() {
                        let v = unsafe { *args_ptr.add(i) };
                        let bits = v.to_bits();
                        let tag = (bits >> 48) as u16;
                        if tag == 0x7FFD {
                            (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader
                        } else {
                            std::ptr::null()
                        }
                    } else {
                        std::ptr::null()
                    }
                };
                match method_name {
                    "length" => {
                        return crate::typedarray::js_typed_array_length(ta) as f64;
                    }
                    "at" => {
                        return crate::typedarray::js_typed_array_at(ta, arg0());
                    }
                    "sort" => {
                        let cmp = arg_closure(0);
                        let result = if cmp.is_null() {
                            crate::typedarray::js_typed_array_sort_default(ta)
                        } else {
                            crate::typedarray::js_typed_array_sort_with_comparator(ta, cmp)
                        };
                        return f64::from_bits(result as u64);
                    }
                    "toSorted" => {
                        let cmp = arg_closure(0);
                        let result = if cmp.is_null() {
                            crate::typedarray::js_typed_array_to_sorted_default(ta)
                        } else {
                            crate::typedarray::js_typed_array_to_sorted_with_comparator(ta, cmp)
                        };
                        return f64::from_bits(result as u64);
                    }
                    "toReversed" => {
                        let result = crate::typedarray::js_typed_array_to_reversed(ta);
                        return f64::from_bits(result as u64);
                    }
                    "with" => {
                        let idx = arg0();
                        let val = if args_len >= 2 && !args_ptr.is_null() {
                            unsafe { *args_ptr.add(1) }
                        } else {
                            f64::NAN
                        };
                        let result = crate::typedarray::js_typed_array_with(ta, idx, val);
                        return f64::from_bits(result as u64);
                    }
                    "findLast" => {
                        let cb = arg_closure(0);
                        if cb.is_null() {
                            return f64::from_bits(crate::value::TAG_UNDEFINED);
                        }
                        return crate::typedarray::js_typed_array_find_last(ta, cb);
                    }
                    "findLastIndex" => {
                        let cb = arg_closure(0);
                        if cb.is_null() {
                            return -1.0;
                        }
                        return crate::typedarray::js_typed_array_find_last_index(ta, cb);
                    }
                    _ => {
                        // Fall through. Other methods aren't handled here
                        // yet; they hit the primitive-method catch-all
                        // below — better than silent no-op.
                    }
                }
            }
        }
    }

    // Issue #514 followup: string method dispatch on any-typed receivers.
    // When `(s: any).at(-1)` / `.slice(1)` / etc. lower through the
    // dispatch tower and `s` actually holds a string, we need to route
    // to the matching `js_string_*` runtime helper. Without this, the
    // primitive-method TypeError catch-all (issue #510 fix below) fires
    // for every legitimate string method call on a `(s: any)` parameter,
    // breaking hono's `mergePath` template-literal logic that mixes
    // `s?.[0]` (handled by `js_dyn_index_get`, issue #514) with
    // `s?.at(-1)` and `s?.slice(1)`. Static call sites for typed string
    // receivers continue to use the inline `js_string_*` paths in
    // `lower_string_method.rs`; this dispatch only catches fallthroughs
    // where codegen couldn't statically prove the type.
    if jsval.is_string() || jsval.is_short_string() {
        let s_ptr = crate::value::js_get_string_pointer_unified(object_handle.get_nanbox_f64())
            as *const crate::StringHeader;
        if !s_ptr.is_null() {
            let s_handle = root_scope.root_string_ptr(s_ptr);
            let receiver_string = || s_handle.get_raw_const_ptr::<crate::StringHeader>();
            let arg_at = |i: usize| -> Option<f64> {
                if i < args_len {
                    arg_handles.get(i).map(|handle| handle.get_nanbox_f64())
                } else {
                    None
                }
            };
            let arg_i32 = |i: usize| -> i32 {
                if let Some(v) = arg_at(i) {
                    if v.is_nan() || v.is_infinite() {
                        0
                    } else {
                        v as i32
                    }
                } else {
                    0
                }
            };
            match method_name {
                "export" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    // Minimal asymmetric KeyObject-surrogate export surface.
                    // The native crypto layer stores PEM-backed RSA/EC keys
                    // and internal Ed/X surrogates as heap strings. For the
                    // high-value Node parity shape (`format: "pem"`), the
                    // stored string is already the exported representation.
                    return object;
                }
                "equals" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    if args_len == 0 || args_ptr.is_null() {
                        return f64::from_bits(JSValue::bool(false).bits());
                    }
                    let other = unsafe { *args_ptr };
                    let other_ptr = crate::value::js_get_string_pointer_unified(other)
                        as *const crate::StringHeader;
                    if other_ptr.is_null()
                        || crate::buffer::asymmetric_key_meta(other_ptr as usize).is_none()
                    {
                        return f64::from_bits(JSValue::bool(false).bits());
                    }
                    let eq = crate::string::js_string_equals(s_ptr, other_ptr) != 0;
                    return f64::from_bits(JSValue::bool(eq).bits());
                }
                "at" => {
                    return crate::string::js_string_at(s_ptr, arg_i32(0));
                }
                "charAt" => {
                    let result = crate::string::js_string_char_at(s_ptr, arg_i32(0));
                    if result.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "charCodeAt" => {
                    return crate::string::js_string_char_code_at(s_ptr, arg_i32(0));
                }
                "slice" => {
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                    let end = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                    let result = crate::string::js_string_slice(s_ptr, start, end);
                    if result.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(result).bits());
                }
                "toString" | "valueOf" => return object_handle.get_nanbox_f64(),
                // Issue #519 follow-up: hono's matcher.js does
                // `path2.match(matcher[0])` where `path2` is a string and
                // `matcher[0]` is a regex. The HIR optimistic
                // `Expr::StringMatch` lowering only fires when the regex
                // arg is a literal or a static `RegExp`-typed Ident — for
                // a `Member` or `Element` access (matcher[0]) it falls
                // through to the dynamic dispatch, which then ended up at
                // the issue #510 catch-all (`(string).match is not a
                // function`) because no runtime arm handled `match`.
                "match" | "matchAll" => {
                    if args_len >= 1 && !args_ptr.is_null() {
                        let regex_val = unsafe { *args_ptr };
                        // Extract regex handle from the arg value. RegExp
                        // values are NaN-boxed pointers; pass through the
                        // pointer extraction the same way the HIR-level
                        // StringMatch path does.
                        let regex_jsval = JSValue::from_bits(regex_val.to_bits());
                        if !regex_jsval.is_pointer() {
                            return f64::from_bits(JSValue::null().bits());
                        }
                        let regex_ptr = regex_jsval.as_pointer::<crate::regex::RegExpHeader>();
                        let result_ptr = if method_name == "match" {
                            crate::regex::js_string_match(s_ptr, regex_ptr)
                        } else {
                            crate::regex::js_string_match_all(s_ptr, regex_ptr)
                        };
                        if result_ptr.is_null() {
                            return f64::from_bits(JSValue::null().bits());
                        }
                        return f64::from_bits(JSValue::pointer(result_ptr as *mut u8).bits());
                    }
                    return f64::from_bits(JSValue::null().bits());
                }
                "search" => {
                    if args_len >= 1 && !args_ptr.is_null() {
                        let regex_val = unsafe { *args_ptr };
                        let regex_jsval = JSValue::from_bits(regex_val.to_bits());
                        if !regex_jsval.is_pointer() {
                            return f64::from_bits(JSValue::int32(-1).bits());
                        }
                        let regex_ptr = regex_jsval.as_pointer::<crate::regex::RegExpHeader>();
                        let i32_v = crate::regex::js_string_search_regex(s_ptr, regex_ptr);
                        return f64::from_bits(JSValue::int32(i32_v).bits());
                    }
                    return f64::from_bits(JSValue::int32(-1).bits());
                }
                // Refs #421 — common string methods on any-typed receivers.
                // Hono's compiled JS (and most npm packages with stripped TS
                // types) does `request.url.indexOf("/")` where `url` is in
                // any-typed position because the type annotation on
                // `(request) =>` was erased at bundle time. Without these
                // arms, the v0.5.593 catch-all throws `(string).indexOf is
                // not a function`. Each arm extracts the search-string
                // argument and calls the existing `js_string_*` runtime
                // helper. Static call sites for typed string receivers keep
                // their inline paths in `lower_string_method.rs` and don't
                // come through this dispatcher.
                "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith" | "concat" => {
                    let arg_str = |i: usize| -> *const crate::StringHeader {
                        if i < args_len && !args_ptr.is_null() {
                            let v = unsafe { *args_ptr.add(i) };
                            crate::value::js_get_string_pointer_unified(v)
                                as *const crate::StringHeader
                        } else {
                            std::ptr::null()
                        }
                    };
                    let needle = arg_str(0);
                    // Integer-returning methods MUST return raw `i as f64` (not
                    // NaN-boxed INT32_TAG) — otherwise downstream comparisons
                    // like `idx < url.length` fail because NaN-boxed values
                    // are NaN and any comparison with NaN returns false. The
                    // typed string-method path in `lower_string_method.rs`
                    // uses `sitofp` (signed-int-to-float) for the same reason.
                    // Boolean-returning methods stay as TAG_TRUE/FALSE since
                    // codegen's `js_is_truthy` and explicit `=== true/false`
                    // checks both unbox these tags correctly (and Node's
                    // `Array.prototype.includes` etc. on plain values
                    // already use this representation).
                    if needle.is_null() {
                        // Match Node: `s.indexOf(undefined)` → -1, includes → false.
                        return match method_name {
                            "indexOf" | "lastIndexOf" => -1.0_f64,
                            "includes" | "startsWith" | "endsWith" => {
                                f64::from_bits(JSValue::bool(false).bits())
                            }
                            "concat" => f64::from_bits(
                                JSValue::string_ptr(s_ptr as *mut crate::StringHeader).bits(),
                            ),
                            _ => f64::from_bits(JSValue::undefined().bits()),
                        };
                    }
                    return match method_name {
                        "indexOf" => {
                            let from = if args_len >= 2 { arg_i32(1) } else { 0 };
                            crate::string::js_string_index_of_from(s_ptr, needle, from) as f64
                        }
                        "includes" => {
                            let from = if args_len >= 2 { arg_i32(1) } else { 0 };
                            let i = crate::string::js_string_index_of_from(s_ptr, needle, from);
                            f64::from_bits(JSValue::bool(i >= 0).bits())
                        }
                        "lastIndexOf" => {
                            crate::string::js_string_last_index_of(s_ptr, needle) as f64
                        }
                        "startsWith" => {
                            let at = if args_len >= 2 { arg_i32(1) } else { 0 };
                            let b = crate::string::js_string_starts_with_at(s_ptr, needle, at);
                            f64::from_bits(JSValue::bool(b != 0).bits())
                        }
                        "endsWith" => {
                            let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                            let at = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                            let b = crate::string::js_string_ends_with_at(s_ptr, needle, at);
                            f64::from_bits(JSValue::bool(b != 0).bits())
                        }
                        "concat" => {
                            let r = crate::string::js_string_concat(s_ptr, needle);
                            f64::from_bits(JSValue::string_ptr(r).bits())
                        }
                        _ => f64::from_bits(JSValue::undefined().bits()),
                    };
                }
                "toUpperCase" => {
                    let r = crate::string::js_string_to_upper_case(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "toLowerCase" => {
                    let r = crate::string::js_string_to_lower_case(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trim" => {
                    let r = crate::string::js_string_trim(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trimStart" | "trimLeft" => {
                    let r = crate::string::js_string_trim_start(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "trimEnd" | "trimRight" => {
                    let r = crate::string::js_string_trim_end(s_ptr);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "substring" => {
                    let len_i32 = unsafe { (*s_ptr).byte_len } as i32;
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let end = if args_len >= 2 { arg_i32(1) } else { len_i32 };
                    let r = crate::string::js_string_substring(s_ptr, start, end);
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "repeat" => {
                    let n = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let r = crate::string::js_string_repeat(s_ptr, n);
                    if r.is_null() {
                        return f64::from_bits(JSValue::undefined().bits());
                    }
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                "split" => {
                    let sep_handle = root_string_arg_handle(&root_scope, &arg_handles, 0);
                    // Issue #567: optional 2nd arg `limit`.
                    let limit = if let Some(v) = arg_at(1) {
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_undefined() || jsv.is_null() {
                            -1
                        } else {
                            let n = crate::builtins::js_number_coerce(
                                arg_handles
                                    .get(1)
                                    .map(|handle| handle.get_nanbox_f64())
                                    .unwrap_or(v),
                            );
                            if n.is_nan() || n < 0.0 {
                                0
                            } else if n > i32::MAX as f64 {
                                i32::MAX
                            } else {
                                n as i32
                            }
                        }
                    } else {
                        -1
                    };
                    let sep = sep_handle
                        .as_ref()
                        .map(|handle| handle.get_raw_const_ptr::<crate::StringHeader>())
                        .unwrap_or(std::ptr::null());
                    let arr = crate::string::js_string_split_n(receiver_string(), sep, limit);
                    return f64::from_bits(JSValue::pointer(arr as *mut u8).bits());
                }
                "replace" | "replaceAll" => {
                    // Two-arg shape: (pattern, replacement). pattern can be a
                    // string OR a RegExp; replacement is a string. Function
                    // replacements aren't supported here yet — they need
                    // closure dispatch and aren't on hono's hot path.
                    let pat_handle = root_string_arg_handle(&root_scope, &arg_handles, 0);
                    let repl_handle = root_string_arg_handle(&root_scope, &arg_handles, 1);
                    let pat_str = || {
                        pat_handle
                            .as_ref()
                            .map(|handle| handle.get_raw_const_ptr::<crate::StringHeader>())
                            .unwrap_or(std::ptr::null())
                    };
                    let repl_str = || {
                        repl_handle
                            .as_ref()
                            .map(|handle| handle.get_raw_const_ptr::<crate::StringHeader>())
                            .unwrap_or(std::ptr::null())
                    };
                    // Detect RegExp pattern: NaN-boxed pointer to a RegExpHeader.
                    if let Some(v) = arg_at(0) {
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_pointer() {
                            // Probe whether the pointer is a RegExpHeader by
                            // checking the GC type tag the regex helpers
                            // already validate; if it's not, the regex helper
                            // returns the original string unchanged. The
                            // global flag on the RegExp determines whether
                            // it replaces all or just the first.
                            let regex_ptr = jsv.as_pointer::<crate::regex::RegExpHeader>();
                            // Heuristic: a non-null POINTER_TAG that's not a
                            // string/array (those have different GC type tags)
                            // is treated as a RegExp here. The runtime helper
                            // already validates internally and falls back
                            // safely on mismatch.
                            if !regex_ptr.is_null() {
                                let r = crate::regex::js_string_replace_regex(
                                    receiver_string(),
                                    regex_ptr,
                                    repl_str(),
                                );
                                return f64::from_bits(JSValue::string_ptr(r).bits());
                            }
                        }
                    }
                    let r = if method_name == "replaceAll" {
                        crate::regex::js_string_replace_all_string(
                            receiver_string(),
                            pat_str(),
                            repl_str(),
                        )
                    } else {
                        crate::regex::js_string_replace_string(
                            receiver_string(),
                            pat_str(),
                            repl_str(),
                        )
                    };
                    return f64::from_bits(JSValue::string_ptr(r).bits());
                }
                _ => {} // not a handled string method — fall through to TypeError catch-all
            }
        }
    }

    // Check if this is a handle-based object (small integer, not a real heap pointer)
    // Handles are used by Fastify, ioredis, and other native modules that store
    // objects in a registry and use integer IDs to reference them.
    if jsval.is_pointer() {
        let raw_ptr = jsval.as_pointer::<u8>() as usize;
        if raw_ptr > 0 && raw_ptr < 0x100000 {
            // This is a handle, not a real memory pointer - dispatch to stdlib
            if let Some(dispatch) = handle_method_dispatch() {
                return dispatch(
                    raw_ptr as i64,
                    method_name.as_ptr(),
                    method_name.len(),
                    args_ptr,
                    args_len,
                );
            }
            // No dispatcher registered, return undefined
            return f64::from_bits(0x7FF8_0000_0000_0001);
        }

        // Guard: null pointer (raw_ptr == 0) means null POINTER_TAG (0x7FFD_0000_0000_0000)
        // Produced by codegen bugs (uninitialized I64 NaN-boxed). Return undefined instead of crashing.
        if raw_ptr == 0 {
            eprintln!(
                "[NULL_PTR_METHOD_CALL] js_native_call_method: null pointer object for method '{}'",
                method_name
            );
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }

        // Buffer / Uint8Array dispatch — buffers are allocated raw without
        // a GcHeader, so the GC type check below would read random bytes
        // before the buffer storage and may accidentally match GC_TYPE_OBJECT.
        // Detect buffers via the BUFFER_REGISTRY first and route through the
        // dedicated dispatcher.
        if crate::buffer::is_registered_buffer(raw_ptr) {
            return dispatch_buffer_method(raw_ptr, method_name, args_ptr, args_len);
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
                match method_name {
                    "map" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_map(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "filter" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_filter(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
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
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        crate::array::js_array_forEach(arr, cb_ptr);
                        return f64::from_bits(crate::value::TAG_UNDEFINED);
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
                        let arg_i32 = |i: usize| -> i32 {
                            if i < args_len && !args_ptr.is_null() {
                                let v = *args_ptr.add(i);
                                if v.is_nan() || v.is_infinite() {
                                    0
                                } else {
                                    v as i32
                                }
                            } else {
                                0
                            }
                        };
                        let len = crate::array::js_array_length(arr) as i32;
                        let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                        let end = if args_len >= 2 { arg_i32(1) } else { len };
                        let result = crate::array::js_array_slice(arr, start, end);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
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
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
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
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_some(arr, cb_ptr);
                    }
                    "every" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_every(arr, cb_ptr);
                    }
                    "find" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_find(arr, cb_ptr);
                    }
                    "findIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_findIndex(arr, cb_ptr);
                        return idx as f64;
                    }
                    "findLast" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        return crate::array::js_array_find_last(arr, cb_ptr);
                    }
                    "findLastIndex" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let idx = crate::array::js_array_find_last_index(arr, cb_ptr);
                        return idx as f64;
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
                        let result = if args_len >= 1 && !args_ptr.is_null() {
                            let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                            let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                            crate::array::js_array_sort_with_comparator(arr, cb_ptr)
                        } else {
                            crate::array::js_array_sort_default(arr)
                        };
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "reverse" => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_reverse(arr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "reduce" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return crate::array::js_array_reduce(arr, cb_ptr, has_init, init);
                    }
                    "reduceRight" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let (has_init, init) = if args_len >= 2 {
                            (1i32, *args_ptr.add(1))
                        } else {
                            (0i32, f64::from_bits(crate::value::TAG_UNDEFINED))
                        };
                        return crate::array::js_array_reduce_right(arr, cb_ptr, has_init, init);
                    }
                    "flat" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let result = crate::array::js_array_flat(arr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "flatMap" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let cb_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let cb_ptr = cb_bits as *const crate::closure::ClosureHeader;
                        let result = crate::array::js_array_flatMap(arr, cb_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "concat" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let other_bits = (*args_ptr).to_bits() & 0x0000_FFFF_FFFF_FFFF;
                        let other_ptr = other_bits as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_concat(arr, other_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "indexOf" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        return crate::array::js_array_indexOf_jsvalue(arr, value) as f64;
                    }
                    "includes" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let value = *args_ptr;
                        let r = crate::array::js_array_includes_jsvalue(arr, value);
                        return f64::from_bits(JSValue::bool(r != 0).bits());
                    }
                    "at" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        return crate::array::js_array_at(arr, *args_ptr);
                    }
                    "fill" if args_len >= 1 && !args_ptr.is_null() => {
                        let arr = raw_ptr as *mut crate::array::ArrayHeader;
                        let result = crate::array::js_array_fill(arr, *args_ptr);
                        return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
                    }
                    "join" => {
                        let arr = raw_ptr as *const crate::array::ArrayHeader;
                        let sep_ptr = if args_len >= 1 && !args_ptr.is_null() {
                            let bits = (*args_ptr).to_bits();
                            let tag = bits >> 48;
                            if tag == 0x7FFF || tag == 0x7FFE {
                                // STRING_TAG or SHORT_STRING tag
                                (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::string::StringHeader
                            } else {
                                std::ptr::null()
                            }
                        } else {
                            std::ptr::null()
                        };
                        let s = crate::array::js_array_join(arr, sep_ptr);
                        return f64::from_bits(JSValue::string_ptr(s).bits());
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
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
            }
            // Issue #1206: Buffer iterators returned from `buf.values()` etc.
            // have a dedicated class id so `.next()` lands here and dispatches
            // to the iterator-protocol helper without paying the generic
            // closure-field scan below.
            if (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID {
                return crate::buffer::dispatch_buffer_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }

            // Scan object fields for a callable property (closure stored via IndexSet)
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_key = crate::string::js_string_from_bytes(
                            method_name.as_ptr(),
                            method_name.len() as u32,
                        );
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if key_val.is_string() {
                                let stored_key = key_val.as_string_ptr();
                                if crate::string::js_string_equals(method_key, stored_key) != 0 {
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
                                    return result;
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup for class instances — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = jsval.as_pointer::<u8>() as i64;
                    return call_vtable_method(func_ptr, this_i64, args_ptr, args_len, param_count);
                }
                if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                    if let Some(ref reg) = *registry {
                        // Refs #420: walk the parent chain via the class
                        // registry. Per JS spec, `subInstance.method()` for
                        // a method defined on a parent dispatches to the
                        // parent's implementation — drizzle's
                        // `serial("id").primaryKey()` where primaryKey is on
                        // ColumnBuilder (grandparent) but the receiver is a
                        // PgSerialBuilder (grandchild). The codegen-side
                        // dispatch tower in `lower_call.rs` only registers
                        // classes the importing module knows about; for
                        // not-by-name-imported subclasses (return values of
                        // imported functions) we depend on this runtime walk.
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
                                    );
                                    let this_i64 = jsval.as_pointer::<u8>() as i64;
                                    return call_vtable_method(
                                        entry.func_ptr,
                                        this_i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                    );
                                }
                            }
                            // Issue #711 part 2: if this class id has a
                            // registered prototype object (from
                            // `Function.prototype = X`), look up the
                            // method as a regular property of that
                            // object. Effect's `EffectPrototype.pipe()`
                            // and friends are own-properties of the
                            // proto object; the value is a closure that
                            // expects `this = receiver`.
                            let proto_obj = class_prototype_object(cur_cid);
                            if !proto_obj.is_null() {
                                let method_key = crate::string::js_string_from_bytes(
                                    method_name.as_ptr(),
                                    method_name.len() as u32,
                                );
                                let field_val = js_object_get_field_by_name(
                                    proto_obj as *const _,
                                    method_key as *const crate::StringHeader,
                                );
                                if !field_val.is_undefined() && !field_val.is_null() {
                                    let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                                    let result = crate::closure::js_native_call_value(
                                        f64::from_bits(field_val.bits()),
                                        args_ptr,
                                        args_len,
                                    );
                                    IMPLICIT_THIS.with(|c| c.set(prev_this));
                                    return result;
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
                if let Some(field_val) =
                    resolve_proto_chain_field(class_id, method_key as *const crate::StringHeader)
                {
                    if !field_val.is_undefined() && !field_val.is_null() {
                        let prev_this = IMPLICIT_THIS.with(|c| c.replace(jsval.bits()));
                        let result = crate::closure::js_native_call_value(
                            f64::from_bits(field_val.bits()),
                            args_ptr,
                            args_len,
                        );
                        IMPLICIT_THIS.with(|c| c.set(prev_this));
                        return result;
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
                    return result;
                }
            }
        }
    }

    // Check Map/Set registries for raw or NaN-boxed pointers.
    // Maps/Sets are allocated with plain alloc (no GcHeader), so they can't be
    // dispatched through the ObjectHeader path below.
    {
        let check_ptr = if jsval.is_pointer() {
            (raw_bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else if !object.is_nan() && raw_bits >= 0x100000 && (raw_bits >> 48) == 0 {
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
                return match method_name {
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
                    "entries" => f64::from_bits(
                        JSValue::pointer(crate::map::js_map_entries(map) as *mut u8).bits(),
                    ),
                    "keys" => f64::from_bits(
                        JSValue::pointer(crate::map::js_map_keys(map) as *mut u8).bits(),
                    ),
                    "values" => f64::from_bits(
                        JSValue::pointer(crate::map::js_map_values(map) as *mut u8).bits(),
                    ),
                    "forEach" if !args.is_empty() => {
                        crate::map::js_map_foreach(map, args[0]);
                        f64::from_bits(crate::value::TAG_UNDEFINED)
                    }
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            if crate::set::is_registered_set(check_ptr) {
                let set = check_ptr as *mut crate::set::SetHeader;
                let args = if !args_ptr.is_null() && args_len > 0 {
                    std::slice::from_raw_parts(args_ptr, args_len)
                } else {
                    &[]
                };
                return match method_name {
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
                    _ => f64::from_bits(crate::value::TAG_UNDEFINED),
                };
            }
            // Buffer / Uint8Array dispatch — allocated raw, not behind a
            // GcHeader, so it can't be discovered through the ObjectHeader
            // path below. Tracked in BUFFER_REGISTRY. Routes Node-style
            // numeric read/write/search/swap method family through
            // `crate::buffer` helpers.
            if crate::buffer::is_registered_buffer(check_ptr) {
                return dispatch_buffer_method(check_ptr, method_name, args_ptr, args_len);
            }
        }
    }

    // Handle raw pointer values without NaN-box tags.
    // Perry sometimes bitcasts I64 pointers to F64 without NaN-boxing (POINTER_TAG).
    // These appear as subnormal floats with bits in the valid heap address range
    // (0x100000 .. 0x0000_FFFF_FFFF_FFFF, upper 16 bits = 0).
    if !jsval.is_pointer() && !object.is_nan() && raw_bits >= 0x100000 && (raw_bits >> 48) == 0 {
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
                return dispatch_native_module_method(obj, method_name, args_ptr, args_len);
            }
            // Issue #1206: same class-id check as the NaN-boxed path above
            // so a raw-pointer iterator value (uncommon, but possible after
            // a bitcast) still routes through the iterator dispatcher.
            if (*obj).class_id == crate::buffer::BUFFER_ITERATOR_CLASS_ID {
                return crate::buffer::dispatch_buffer_iterator_method(
                    obj as *mut ObjectHeader,
                    method_name,
                );
            }

            // Field name scan on this object
            let keys = (*obj).keys_array;
            if !keys.is_null() {
                let keys_ptr = keys as usize;
                if (keys_ptr as u64) >> 48 == 0 && keys_ptr >= 0x10000 {
                    let key_count = crate::array::js_array_length(keys) as usize;
                    if key_count <= 65536 {
                        let method_key = crate::string::js_string_from_bytes(
                            method_name.as_ptr(),
                            method_name.len() as u32,
                        );
                        for i in 0..key_count {
                            let key_val = crate::array::js_array_get(keys, i as u32);
                            if key_val.is_string() {
                                let stored_key = key_val.as_string_ptr();
                                if crate::string::js_string_equals(method_key, stored_key) != 0 {
                                    let field_val = js_object_get_field(obj as *mut _, i as u32);
                                    if field_val.is_pointer() {
                                        return crate::closure::js_native_call_value(
                                            f64::from_bits(field_val.bits()),
                                            args_ptr,
                                            args_len,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Vtable lookup — fast path via per-callsite IC
            let class_id = (*obj).class_id;
            if class_id != 0 {
                if let Some((func_ptr, param_count)) =
                    vtable_ic_lookup(class_id, method_name_ptr as usize)
                {
                    let this_i64 = raw_bits as i64;
                    return call_vtable_method(func_ptr, this_i64, args_ptr, args_len, param_count);
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
                                    );
                                    let this_i64 = raw_bits as i64;
                                    return call_vtable_method(
                                        entry.func_ptr,
                                        this_i64,
                                        args_ptr,
                                        args_len,
                                        entry.param_count,
                                    );
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

    // Handle common method calls
    match method_name {
        // Function.prototype.bind - returns the same function for native closures
        // This is a simplification - real bind() creates a new function with bound 'this'
        "bind" => {
            // For native closures, we return the function as-is
            // The 'this' binding is handled at the call site
            return object;
        }

        // `obj.hasOwnProperty(key)` — duck-types as truthy for any
        // non-null/undefined receiver where the field-scan and class
        // dispatch above couldn't find a user-defined override. Walking
        // the actual key set on every shape (ObjectHeader fields,
        // closure dynamic props, array keys, …) is more work than this
        // entry point is meant to do; ramda's `_clone` / `_has` only
        // need a non-throwing return so the surrounding pattern doesn't
        // fall into the spec gap. Pre-fix, the chained
        // `Object.prototype.hasOwnProperty.call(obj, key)` reads
        // `Object.prototype.hasOwnProperty` as `undefined` from the
        // empty proto and threw `value is not a function` at module
        // init in `_clone.js` / `_isArguments.js`.
        "hasOwnProperty" => {
            if jsval.is_undefined() || jsval.is_null() {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            return f64::from_bits(JSValue::bool(true).bits());
        }

        // `obj.propertyIsEnumerable(key)` — same shape as
        // `hasOwnProperty`. Spec says true for own enumerable
        // properties (the typical case for object literals). Without
        // walking the receiver's keys, we approximate as
        // `truthy receiver → true` — matches Node for ramda's
        // `keys.js` IIFE (`!{toString:null}.propertyIsEnumerable('toString')`
        // expects `true`, so `hasEnumBug` resolves to `false`).
        // Arguments-like receivers also return true here, which
        // matches the legacy non-Safari behavior ramda's IIFE checks
        // against.
        "propertyIsEnumerable" => {
            if jsval.is_undefined() || jsval.is_null() {
                return f64::from_bits(JSValue::bool(false).bits());
            }
            return f64::from_bits(JSValue::bool(true).bits());
        }

        // Function.prototype.call(thisArg, ...args) — invoke the receiver
        // closure with `thisArg` bound as `this` and the remaining args
        // passed positionally. Ramda's curry helpers (`_curry1`, `_curry2`,
        // `_curry3`) build their dispatch chain around
        // `fn.apply(this, arguments)` / `fn.call(this, x)`, so without these
        // arms ramda fails immediately on the first curried export.
        "call" if jsval.is_pointer() => {
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(raw_ptr) {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let rest_ptr = if args_len > 1 && !args_ptr.is_null() {
                    args_ptr.add(1)
                } else {
                    std::ptr::null()
                };
                let rest_len = if args_len > 1 { args_len - 1 } else { 0 };
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                let result = crate::closure::js_native_call_value(object, rest_ptr, rest_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
        }

        // Function.prototype.apply(thisArg, argsArray) — invoke the receiver
        // closure with `thisArg` bound as `this` and the elements of
        // `argsArray` spread as positional arguments. `argsArray` may be
        // null / undefined (treat as no args). Mirrors `js_native_call_method_apply`
        // but for the `Function.prototype.apply` path rather than the
        // dynamic-spread method-call codegen path.
        "apply" if jsval.is_pointer() => {
            let raw_ptr = (object.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
            if crate::closure::is_closure_ptr(raw_ptr) {
                let this_arg = if args_len >= 1 && !args_ptr.is_null() {
                    *args_ptr
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let args_arr_val = if args_len >= 2 && !args_ptr.is_null() {
                    *args_ptr.add(1)
                } else {
                    f64::from_bits(crate::value::TAG_UNDEFINED)
                };
                let args_arr_jsval = JSValue::from_bits(args_arr_val.to_bits());
                let buf: Vec<f64> = if args_arr_jsval.is_pointer() {
                    let arr_ptr = (args_arr_val.to_bits() & 0x0000_FFFF_FFFF_FFFF)
                        as *const crate::array::ArrayHeader;
                    if arr_ptr.is_null() {
                        Vec::new()
                    } else {
                        let n = crate::array::js_array_length(arr_ptr) as usize;
                        (0..n)
                            .map(|i| crate::array::js_array_get_f64(arr_ptr, i as u32))
                            .collect()
                    }
                } else {
                    Vec::new()
                };
                let (call_args_ptr, call_args_len) = if buf.is_empty() {
                    (std::ptr::null::<f64>(), 0_usize)
                } else {
                    (buf.as_ptr(), buf.len())
                };
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(this_arg.to_bits()));
                let result =
                    crate::closure::js_native_call_value(object, call_args_ptr, call_args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
        }

        // Common string methods on string values
        "toString" => {
            if jsval.is_string() {
                return object;
            } else if jsval.is_bigint() {
                let ptr = jsval.as_bigint_ptr();
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_number() {
                let n = jsval.as_number();
                let s = if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
                    (n as i64).to_string()
                } else {
                    n.to_string()
                };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_bool() {
                let s = if jsval.as_bool() { "true" } else { "false" };
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_undefined() {
                let s = "undefined";
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            } else if jsval.is_null() {
                let s = "null";
                let str_ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
                return f64::from_bits(JSValue::string_ptr(str_ptr).bits());
            }
        }

        // Array methods - delegate to array runtime
        "push" if jsval.is_pointer() => {
            let arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            if args_len > 0 && !args_ptr.is_null() {
                let val = *args_ptr;
                crate::array::js_array_push_f64(arr, val);
            }
            return crate::array::js_array_length(arr) as f64;
        }
        "pop" if jsval.is_pointer() => {
            let arr =
                jsval.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
            return crate::array::js_array_pop_f64(arr);
        }
        "length" if jsval.is_pointer() => {
            let arr = jsval.as_pointer::<crate::array::ArrayHeader>();
            return crate::array::js_array_length(arr) as f64;
        }

        _ => {}
    }

    // If it's an object with a method stored as a closure in a field,
    // try to find and call it
    if jsval.is_pointer() {
        let obj = jsval.as_pointer::<ObjectHeader>();

        // Validate this is an ObjectHeader, not some other heap type.
        // Check GcHeader first (reliable for heap objects), then fallback to ObjectHeader.object_type
        // for static/const objects that don't have GcHeaders.
        // Guard: ensure we can safely read GC_HEADER_SIZE bytes before obj
        if (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
            return 0.0;
        }
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let gc_type = (*gc_header).obj_type;

        // Issue #618: closure receivers (GC_TYPE_CLOSURE=4 OR
        // CLOSURE_MAGIC-marked GC_TYPE_OBJECT slot) — look up the method
        // name in the closure's dynamic-prop side-table. If a callable
        // closure is stored there (via the IIFE-namespace pattern
        // `((sql2) => { sql2.identifier = ...; })(sql)`), dispatch
        // through `js_native_call_value`. Pre-fix this path returned the
        // NULL_OBJECT_BYTES stub for any method call on a closure, so
        // the call result was an empty object stub instead of the
        // dynamic-prop closure's return value.
        let is_closure = gc_type == crate::gc::GC_TYPE_CLOSURE
            || *((obj as *const u8).add(12) as *const u32) == crate::closure::CLOSURE_MAGIC;
        if is_closure {
            let dyn_val = crate::closure::closure_get_dynamic_prop(obj as usize, method_name);
            if dyn_val.to_bits() != crate::value::TAG_UNDEFINED {
                let recv_bits = jsval.bits();
                let prev_this = IMPLICIT_THIS.with(|c| c.replace(recv_bits));
                let result = crate::closure::js_native_call_value(dyn_val, args_ptr, args_len);
                IMPLICIT_THIS.with(|c| c.set(prev_this));
                return result;
            }
            let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
            return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
        }

        // #1387: synthesized `PerformanceEntry#toJSON()`. Entry objects are
        // plain shaped objects with no stored `toJSON` field, so the
        // field-scan dispatch below would miss it. A bound-method read (from
        // the property-get intercept) routes here via `dispatch_bound_method`,
        // and a direct `entry.toJSON()` call lands here too — both serialize
        // the entry's fields into a plain object. Safe to read the header:
        // `obj` is a validated heap object (gc_type read above).
        if method_name == "toJSON"
            && gc_type == crate::gc::GC_TYPE_OBJECT
            && crate::perf_hooks::is_perf_entry_object(obj)
        {
            return crate::perf_hooks::perf_entry_to_json(object);
        }

        if gc_type != crate::gc::GC_TYPE_OBJECT {
            // Only accept object_type == 1 (OBJECT_TYPE_REGULAR)
            let object_type = (*obj).object_type;
            // Closes #645: when a method falls through every dispatcher
            // and returns NULL_OBJECT_BYTES (e.g. drizzle's
            // `this.client.prepare(...)` where `this.client` resolved to
            // a heap-object that doesn't dispatch any method named
            // "prepare"), the result gets stored as `this.stmt` and the
            // chained `this.stmt.raw().all(...)` re-enters this function
            // with `obj` pointing at NULL_OBJECT_BYTES — a static stub in
            // the binary's data segment, NOT the macOS userspace heap
            // range that `is_valid_obj_ptr` requires (HEAP_MIN ==
            // 0x200_0000_0000). Pre-fix this returned a literal `0.0`,
            // which the codegen interprets as the IEEE-754 number zero,
            // so the next chained method saw a number receiver and
            // threw `(number).<method> is not a function`. Returning the
            // null-object stub matches every other catch-all in this
            // function and keeps `typeof === "object"` so chained
            // operations propagate consistently instead of mid-chain
            // numeric arithmetic on bit patterns. Truly garbage pointers
            // benefit too — chained calls hit a stable null stub instead
            // of mysterious numeric values.
            if !is_valid_obj_ptr(obj as *const u8) {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            if object_type != crate::error::OBJECT_TYPE_REGULAR {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
        }

        let keys = (*obj).keys_array;

        if !keys.is_null() {
            // Validate keys_array pointer before dereferencing
            let keys_ptr = keys as usize;
            if (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            // Issue #62 phase B: removed macOS "ASCII-like pointer" heuristic —
            // mimalloc + arena strings produce valid heap pointers with bytes
            // 32-39 in the 0x20-0x7E range, causing false positives. The call
            // into `js_object_get_field_by_name` below performs its own
            // GcHeader-based validation.

            // Search for the method in the object's fields
            let key_count = crate::array::js_array_length(keys) as usize;
            // Sanity check key_count
            if key_count > 65536 {
                let null_obj_ptr = &NULL_OBJECT_BYTES as *const NullObjectBytes as *mut u8;
                return f64::from_bits(JSValue::pointer(null_obj_ptr).bits());
            }
            // Compare method_name bytes directly against each stored key
            // instead of allocating a transient StringHeader via
            // js_string_from_bytes — that allocation showed up as ~10% of
            // perf-comprehensive's hot-path samples (one alloc per
            // dynamic-dispatch method call × N keys-array lookups).
            let method_bytes = method_name.as_bytes();
            for i in 0..key_count {
                let key_val = crate::array::js_array_get(keys, i as u32);
                if key_val.is_string() {
                    let stored_key = key_val.as_string_ptr();
                    let matches = if !crate::string::is_valid_string_ptr(stored_key) {
                        false
                    } else {
                        let blen = (*stored_key).byte_len as usize;
                        if blen != method_bytes.len() {
                            false
                        } else {
                            let stored_data = crate::string::string_data(stored_key);
                            let stored = std::slice::from_raw_parts(stored_data, blen);
                            stored == method_bytes
                        }
                    };
                    if matches {
                        // Found the method — delegate to `js_native_call_value`
                        // which handles both NaN-boxed pointers (POINTER_TAG)
                        // and raw-pointer-bits (e.g. the resolve/reject
                        // closures from `js_promise_new_with_executor`,
                        // transmuted `i64 → f64` so their bits live outside
                        // the NaN range). The earlier `is_pointer()` gate
                        // bailed on the raw-pointer case: `{ resolve }` on a
                        // plain object caused `box.resolve(x)` to land here,
                        // the tag check failed, we fell through to vtable
                        // lookup, and returned NULL_OBJECT_BYTES without
                        // invoking `js_promise_resolve` → the awaiter hung
                        // forever (issue #87). `js_native_call_value`
                        // validates CLOSURE_MAGIC before calling the func
                        // pointer, so non-callable field values (numbers,
                        // strings, booleans) safely return undefined.
                        let field_val = js_object_get_field(obj as *mut _, i as u32);
                        return crate::closure::js_native_call_value(
                            f64::from_bits(field_val.bits()),
                            args_ptr,
                            args_len,
                        );
                    }
                }
            }
        }

        // Vtable lookup: check if this class has a registered method in the vtable
        let class_id = (*obj).class_id;
        if class_id != 0 {
            if let Ok(registry) = CLASS_VTABLE_REGISTRY.read() {
                if let Some(ref reg) = *registry {
                    if let Some(vtable) = reg.get(&class_id) {
                        if let Some(entry) = vtable.methods.get(method_name) {
                            let this_i64 = jsval.as_pointer::<u8>() as i64;
                            return call_vtable_method(
                                entry.func_ptr,
                                this_i64,
                                args_ptr,
                                args_len,
                                entry.param_count,
                            );
                        }
                    }
                }
            }
        }
    }

    // Issue #510: throw `TypeError: <expr> is not a function` when
    // the receiver is a non-string primitive (number / int32 / bool /
    // bigint) and dispatch above didn't fire. Node auto-boxes
    // primitives via Number/Boolean/BigInt prototypes; when the
    // prototype lookup yields undefined, the call site throws.
    // Without primitive auto-boxing, Perry must surface the same
    // diagnostic at dispatch time — silently returning the
    // null-object sentinel (the historical fall-through below) lets
    // typo'd method calls run as no-ops, masking real bugs.
    //
    // Strings don't reach this catch-all in the typical case —
    // codegen's `lower_string_method` intercepts string-typed
    // receivers and throws there directly (matching ABI). The string
    // arm is left in here for the rare path where a string flows
    // through dynamic dispatch (e.g. raw NaN-boxed receiver from a
    // Map.get() result the user typed as `any`).
    //
    // Real-object receivers keep the `NULL_OBJECT_BYTES`
    // fall-through. Many existing call paths use this dispatcher as
    // a generic shortcut and rely on the silent null-object return
    // for unknown methods; tightening that is tracked separately.
    //
    // Issue #511: `undefined` / `null` receivers must throw a node-shaped
    // `TypeError: Cannot read properties of <kind> (reading '<method>')`
    // and exit 1. Codegen's `Expr::PropertyGet` lowering already throws
    // on the bare property read (`obj.foo`, issue #462), but the
    // `Call { callee: PropertyGet }` shortcut in `lower_call.rs`
    // routes `obj.foo()` straight to `js_native_call_method` without
    // re-evaluating the receiver through PropertyGet — so the codegen
    // gate never fires for the call form. Without this arm, `x.foo()`
    // on `undefined` silently returned `NULL_OBJECT_BYTES` and the
    // process exited 0, breaking CI gates that rely on non-zero exit
    // for uncaught errors. Earlier toString/bind/push/pop/length match
    // arms intentionally short-circuit before this point so existing
    // Perry code that calls those on `undefined`/`null` keeps working
    // (Perry-ism — Node throws there too, but tightening that breaks
    // unrelated callers; the typo case below is what we want to surface).
    if jsval.is_undefined() || jsval.is_null() {
        let is_null_u32 = if jsval.is_null() { 1u32 } else { 0u32 };
        crate::error::js_throw_type_error_property_access(
            is_null_u32,
            method_name.as_ptr(),
            method_name.len(),
        );
    }
    // Issue #687: INT32-NaN-boxed value whose payload is a registered
    // class id — i.e. a `ClassRef` produced by `Expr::ClassRef` codegen.
    // Effect's `Schema.NonNegative.pipe(int()).annotations({...})` chains
    // produce a ClassRef out of the first `.pipe()` (via the codegen-side
    // defensive no-op in `lower_call.rs::Expr::ClassRef`) and the chained
    // `.annotations(...)` reaches us with that ClassRef as the receiver.
    // Treat it as a chainable no-op: return the receiver so further
    // `.method(...)` calls stay typed-class-shaped during module init.
    // The result isn't semantically equivalent to Effect's transformed
    // schema, but it advances Schema.ts__init past sites that previously
    // threw `(number).<method> is not a function`. Paired with the
    // codegen-side fix in `lower_call.rs` for the simpler
    // `ClassRef.method()` shape.
    if jsval.is_int32() {
        let payload = jsval.as_int32() as u32;
        if payload != 0 {
            let guard = REGISTERED_CLASS_IDS.read().unwrap();
            if let Some(set) = guard.as_ref() {
                if set.contains(&payload) {
                    return object;
                }
            }
        }
    }
    let primitive_kind: Option<&'static str> = if jsval.is_any_string() {
        Some("string")
    } else if jsval.is_int32() || jsval.is_number() {
        Some("number")
    } else if jsval.is_bool() {
        Some("boolean")
    } else if jsval.is_bigint() {
        Some("bigint")
    } else {
        None
    };
    if let Some(kind) = primitive_kind {
        crate::error::js_throw_type_error_not_a_function(
            kind.as_ptr(),
            kind.len(),
            method_name.as_ptr(),
            method_name.len(),
        );
    }

    // Issue #648: real-object receivers also throw when the method
    // doesn't exist anywhere in the dispatch chain (no field-stored
    // closure, no class vtable entry, no prototype walk hit). Pre-fix
    // this catch-all returned `NULL_OBJECT_BYTES` so codegen wouldn't
    // SIGSEGV when it NaN-unboxed the result and dereferenced it as a
    // pointer — but that masked typo'd method calls as silent no-ops
    // and was the single largest source of cascading parity failures
    // (`test_parity_timers` hung waiting on `timers.setTimeout` which
    // silently no-op'd; many other parity tests truncated mid-script
    // when an unimplemented binding's method silently no-op'd inside
    // the surrounding async path). Now we throw the standard `<prop>
    // is not a function` TypeError, which `try`/`catch` catches (per
    // #596's exception-routing fix).
    crate::error::js_throw_type_error_not_a_function(
        std::ptr::null(),
        0,
        method_name.as_ptr(),
        method_name.len(),
    );
}
