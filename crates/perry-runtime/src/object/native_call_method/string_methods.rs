use super::*;

pub(super) unsafe fn dispatch_string(
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
            // NOTE: user-defined `String.prototype` methods on primitive string
            // receivers are routed through the `primitive_kind` fallback below
            // (after native string-method dispatch). Intercepting here, *before*
            // native dispatch, re-enters `js_native_call_method` via the #4100
            // brand-check re-dispatch thunk installed on `String.prototype`
            // (e.g. `replace`), causing unbounded recursion.
            let s_handle = root_scope.root_string_ptr(s_ptr);
            let receiver_string = || s_handle.get_raw_const_ptr::<crate::StringHeader>();
            let arg_at = |i: usize| -> Option<f64> {
                if i < args_len {
                    arg_handles.get(i).map(|handle| handle.get_nanbox_f64())
                } else {
                    None
                }
            };
            // Index/position args follow `ToIntegerOrInfinity` (ToNumber, then
            // truncate, clamping ±Infinity to i32 bounds) so a boolean
            // (`slice(false, true)` → 0,1), numeric string (`"2"`), or `{ valueOf
            // }` object coerces like Node instead of being read as NaN→0. Plain
            // numbers/int32 take the fast path inside the helper. A missing arg
            // is 0 (the per-method default end/length is applied by the arm).
            let arg_i32 = |i: usize| -> i32 {
                match arg_at(i) {
                    Some(v) => crate::string::js_string_index_to_i32(v),
                    None => 0,
                }
            };
            match method_name {
                "toCryptoKey" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    let ptr = crate::value::JS_NATIVE_WEBCRYPTO_DISPATCH
                        .load(std::sync::atomic::Ordering::SeqCst);
                    if ptr.is_null() {
                        return Some(f64::from_bits(JSValue::undefined().bits()));
                    }
                    let dispatch: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64 =
                        std::mem::transmute(ptr);
                    let key_value = f64::from_bits(JSValue::string_ptr(s_ptr as *mut _).bits());
                    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
                    let dispatch_args = [
                        key_value,
                        arg_at(0).unwrap_or(undefined),
                        arg_at(1).unwrap_or(undefined),
                        arg_at(2).unwrap_or(undefined),
                    ];
                    return Some(dispatch(
                        b"keyObjectToCryptoKey".as_ptr(),
                        "keyObjectToCryptoKey".len(),
                        dispatch_args.as_ptr(),
                        dispatch_args.len(),
                    ));
                }
                "export" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    // Minimal asymmetric KeyObject-surrogate export surface.
                    // The native crypto layer stores PEM-backed RSA/EC keys
                    // and internal Ed/X surrogates as heap strings. For the
                    // high-value Node parity shape (`format: "pem"`), the
                    // stored string is already the exported representation.
                    return Some(object);
                }
                "equals" if crate::buffer::asymmetric_key_meta(s_ptr as usize).is_some() => {
                    if args_len == 0 || args_ptr.is_null() {
                        return Some(f64::from_bits(JSValue::bool(false).bits()));
                    }
                    let other = unsafe { *args_ptr };
                    let other_ptr = crate::value::js_get_string_pointer_unified(other)
                        as *const crate::StringHeader;
                    if other_ptr.is_null()
                        || crate::buffer::asymmetric_key_meta(other_ptr as usize).is_none()
                    {
                        return Some(f64::from_bits(JSValue::bool(false).bits()));
                    }
                    let eq = crate::string::js_string_equals(s_ptr, other_ptr) != 0;
                    return Some(f64::from_bits(JSValue::bool(eq).bits()));
                }
                "at" => {
                    return Some(crate::string::js_string_at(s_ptr, arg_i32(0)));
                }
                // `str[Symbol.iterator]()` returns a real String iterator object
                // (codepoint-aware, surrogate pairs collapse to one element) so
                // `Object.getPrototypeOf(''[Symbol.iterator]())` resolves to
                // `%StringIteratorPrototype%` and generic `.next()` drivers work.
                "Symbol.iterator" | "@@iterator" => {
                    return Some(crate::string::string_values_iter(receiver_string()));
                }
                "charAt" => {
                    let result = crate::string::js_string_char_at(s_ptr, arg_i32(0));
                    if result.is_null() {
                        return Some(f64::from_bits(JSValue::undefined().bits()));
                    }
                    return Some(f64::from_bits(JSValue::string_ptr(result).bits()));
                }
                "charCodeAt" => {
                    return Some(crate::string::js_string_char_code_at(s_ptr, arg_i32(0)));
                }
                "slice" => {
                    // Coerce args first (`arg_i32` may run user `valueOf` and move
                    // the receiver under GC), then re-fetch the rooted receiver.
                    // An `undefined` end means `len` (spec), not `ToInteger(0)`.
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let end_arg = match arg_at(1) {
                        Some(v) if !JSValue::from_bits(v.to_bits()).is_undefined() => {
                            Some(arg_i32(1))
                        }
                        _ => None,
                    };
                    let s = receiver_string();
                    let len_i32 = unsafe { (*s).byte_len } as i32;
                    let end = end_arg.unwrap_or(len_i32);
                    let result = crate::string::js_string_slice(s, start, end);
                    if result.is_null() {
                        return Some(f64::from_bits(JSValue::undefined().bits()));
                    }
                    return Some(f64::from_bits(JSValue::string_ptr(result).bits()));
                }
                "toString" | "valueOf" => return Some(object_handle.get_nanbox_f64()),
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
                    // Missing arg ⇒ `undefined` (→ empty `/(?:)/` regex).
                    let _pattern_val =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    #[cfg(feature = "regex-engine")]
                    {
                        let pattern_val = _pattern_val;
                        if method_name == "matchAll" {
                            let result_ptr =
                                crate::regex::js_string_match_all_value(s_ptr, pattern_val);
                            if result_ptr.is_null() {
                                return Some(f64::from_bits(JSValue::null().bits()));
                            }
                            return Some(f64::from_bits(
                                JSValue::pointer(result_ptr as *mut u8).bits(),
                            ));
                        }
                        // Coerce a non-RegExp arg via `RegExpCreate(ToString(arg))`
                        // (a string pattern / `undefined` / `{ toString }` object),
                        // matching the codegen path.
                        let result_ptr = crate::regex::js_string_match_value(s_ptr, pattern_val);
                        if result_ptr.is_null() {
                            return Some(f64::from_bits(JSValue::null().bits()));
                        }
                        return Some(f64::from_bits(
                            JSValue::pointer(result_ptr as *mut u8).bits(),
                        ));
                    }
                    // Engine gated off: a string `.match`/`.matchAll` can only
                    // be reached by a program that uses regex (which forces the
                    // engine on), so this is dead — `null` (no match) is benign.
                    #[cfg(not(feature = "regex-engine"))]
                    return Some(f64::from_bits(JSValue::null().bits()));
                }
                "search" => {
                    let _regex_val =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    #[cfg(feature = "regex-engine")]
                    {
                        let i32_v = crate::regex::js_string_search_value(s_ptr, _regex_val);
                        // Return a RAW `f64` (not NaN-boxed INT32_TAG): a boxed-int
                        // result fails `aString.search(x) === 5` strict-equality
                        // against a plain number literal. Mirrors the `indexOf`
                        // arm's `as f64` convention.
                        return Some(i32_v as f64);
                    }
                    // Engine gated off: dead (see `match` arm) — `-1` (not found).
                    #[cfg(not(feature = "regex-engine"))]
                    return Some(-1.0_f64);
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
                "concat" => {
                    let acc_handle = root_scope.root_string_ptr(receiver_string());
                    for i in 0..args_len {
                        let value = arg_at(i)
                            .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                        let result = crate::string::js_string_concat_value(
                            acc_handle.get_raw_const_ptr::<crate::StringHeader>(),
                            value,
                        );
                        acc_handle.set_raw_const_ptr(result as *const crate::StringHeader);
                    }
                    let result = acc_handle.get_raw_const_ptr::<crate::StringHeader>()
                        as *mut crate::StringHeader;
                    return Some(f64::from_bits(JSValue::string_ptr(result).bits()));
                }
                "indexOf" | "includes" | "lastIndexOf" | "startsWith" | "endsWith" => {
                    let search_arg_to_string = |method_id: i32| -> *const crate::StringHeader {
                        let value = arg_at(0)
                            .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                        crate::string::js_string_search_value_to_string(value, method_id)
                            as *const crate::StringHeader
                    };
                    let needle_raw = match method_name {
                        "includes" => search_arg_to_string(0),
                        "startsWith" => search_arg_to_string(1),
                        "endsWith" => search_arg_to_string(2),
                        // indexOf / lastIndexOf apply `ToString(searchString)` with
                        // no RegExp TypeError: `s.indexOf(undefined)` searches for
                        // "undefined", `s.indexOf({toString(){…}})` uses the result.
                        _ => {
                            let value = arg_at(0)
                                .unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                            crate::value::js_jsvalue_to_string(value) as *const crate::StringHeader
                        }
                    };
                    // ToString above may run user code (object `toString`/`valueOf`)
                    // and move either string under GC — root the needle and re-read
                    // the receiver before the byte-level helpers below.
                    let needle_h = if needle_raw.is_null() {
                        None
                    } else {
                        Some(root_scope.root_string_ptr(needle_raw))
                    };
                    let needle = needle_h
                        .as_ref()
                        .map(|h| h.get_raw_const_ptr::<crate::StringHeader>())
                        .unwrap_or(std::ptr::null());
                    let s_ptr = receiver_string();
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
                        return Some(match method_name {
                            "indexOf" | "lastIndexOf" => -1.0_f64,
                            "includes" | "startsWith" | "endsWith" => {
                                f64::from_bits(JSValue::bool(false).bits())
                            }
                            _ => f64::from_bits(JSValue::undefined().bits()),
                        });
                    }
                    return Some(match method_name {
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
                            if args_len >= 2 {
                                let pos = unsafe { *args_ptr.add(1) };
                                crate::string::js_string_last_index_of_from(s_ptr, needle, pos, 1)
                                    as f64
                            } else {
                                crate::string::js_string_last_index_of(s_ptr, needle) as f64
                            }
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
                        _ => f64::from_bits(JSValue::undefined().bits()),
                    });
                }
                "toUpperCase" => {
                    let r = crate::string::js_string_to_upper_case(s_ptr);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "toLowerCase" => {
                    let r = crate::string::js_string_to_lower_case(s_ptr);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "trim" => {
                    let r = crate::string::js_string_trim(s_ptr);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "trimStart" | "trimLeft" => {
                    let r = crate::string::js_string_trim_start(s_ptr);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "trimEnd" | "trimRight" => {
                    let r = crate::string::js_string_trim_end(s_ptr);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "substring" => {
                    // An `undefined` end means `len` (spec), not `ToInteger(0)`.
                    let start = if args_len >= 1 { arg_i32(0) } else { 0 };
                    let end_arg = match arg_at(1) {
                        Some(v) if !JSValue::from_bits(v.to_bits()).is_undefined() => {
                            Some(arg_i32(1))
                        }
                        _ => None,
                    };
                    let s = receiver_string();
                    let len_i32 = unsafe { (*s).byte_len } as i32;
                    let end = end_arg.unwrap_or(len_i32);
                    let r = crate::string::js_string_substring(s, start, end);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "substr" => {
                    // Legacy substr(start, length): negative start counts from
                    // the end, the 2nd arg is a length, and an `undefined`
                    // length means "rest of string". `js_string_substr` runs
                    // ToIntegerOrInfinity on the raw values itself (start before
                    // length), so pass them through un-coerced (#2897).
                    let undefined = f64::from_bits(JSValue::undefined().bits());
                    let start_val = arg_at(0).unwrap_or(undefined);
                    let length_val = arg_at(1).unwrap_or(undefined);
                    let s = receiver_string();
                    let r = crate::string::js_string_substr(s, start_val, length_val);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "toLocaleLowerCase" => {
                    let locales =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let r = crate::string::js_string_to_locale_lower_case(s_ptr, locales);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "toLocaleUpperCase" => {
                    let locales =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let r = crate::string::js_string_to_locale_upper_case(s_ptr, locales);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "repeat" => {
                    let n = arg_at(0).unwrap_or(0.0);
                    let r = crate::string::js_string_repeat(s_ptr, n);
                    if r.is_null() {
                        return Some(f64::from_bits(JSValue::undefined().bits()));
                    }
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "split" => {
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
                    // `split(undefined)` (or no separator) yields the whole string
                    // as a single element — NOT a per-character split (which is what
                    // an empty-string separator does), and NOT [] (`limit === 0`).
                    let sep_undefined = match arg_at(0) {
                        None => true,
                        Some(v) => JSValue::from_bits(v.to_bits()).is_undefined(),
                    };
                    if sep_undefined {
                        let s = receiver_string();
                        let arr = if limit == 0 {
                            crate::array::js_array_alloc(0)
                        } else {
                            let a = crate::array::js_array_alloc(0);
                            crate::array::js_array_push_f64(
                                a,
                                f64::from_bits(
                                    JSValue::string_ptr(s as *mut crate::StringHeader).bits(),
                                ),
                            )
                        };
                        return Some(f64::from_bits(JSValue::pointer(arr as *mut u8).bits()));
                    }
                    // A RegExp separator must be passed through as its raw pointer so
                    // `js_string_split_n` detects it (by GC header) and delegates to
                    // the regex splitter. Any other value is ToString-coerced.
                    let v0 = arg_at(0).unwrap();
                    let jv0 = JSValue::from_bits(v0.to_bits());
                    let sep_is_regex =
                        jv0.is_pointer() && crate::regex::is_regex_pointer(jv0.as_pointer::<u8>());
                    let (sep, _sep_h) = if sep_is_regex {
                        (jv0.as_pointer::<crate::StringHeader>(), None)
                    } else {
                        let coerced =
                            crate::builtins::js_string_coerce(v0) as *const crate::StringHeader;
                        let h = root_scope.root_string_ptr(coerced);
                        let p = h.get_raw_const_ptr::<crate::StringHeader>();
                        (p, Some(h))
                    };
                    let s = receiver_string();
                    let arr = crate::string::js_string_split_n(s, sep, limit);
                    return Some(f64::from_bits(JSValue::pointer(arr as *mut u8).bits()));
                }
                "replace" | "replaceAll" => {
                    // Two-arg shape: (pattern, replacement). pattern can be a
                    // string OR a RegExp; replacement is a string OR a function.
                    // Function replacements route to the callback helpers so
                    // `str.replace(x, fn)` observes Node's callback argument
                    // shape and receiver binding.
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
                    if let (Some(pat_val), Some(repl_val)) = (arg_at(0), arg_at(1)) {
                        // `pat_jsv` is only consulted by the regex-engine-gated
                        // branch below (RegExp pattern + callback replacer).
                        #[cfg_attr(not(feature = "regex-engine"), allow(unused_variables))]
                        let pat_jsv = JSValue::from_bits(pat_val.to_bits());
                        let repl_jsv = JSValue::from_bits(repl_val.to_bits());
                        if repl_jsv.is_pointer() {
                            let repl_raw = (repl_val.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
                            if crate::closure::is_closure_ptr(repl_raw) {
                                #[cfg(feature = "regex-engine")]
                                if pat_jsv.is_pointer() {
                                    let regex_ptr =
                                        pat_jsv.as_pointer::<crate::regex::RegExpHeader>();
                                    if !regex_ptr.is_null()
                                        && crate::regex::is_regex_pointer(regex_ptr as *const u8)
                                    {
                                        let r = if method_name == "replaceAll" {
                                            crate::regex::js_string_replace_all_regex_fn(
                                                receiver_string(),
                                                regex_ptr,
                                                repl_val,
                                            )
                                        } else {
                                            crate::regex::js_string_replace_regex_fn(
                                                receiver_string(),
                                                regex_ptr,
                                                repl_val,
                                            )
                                        };
                                        return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                                    }
                                }
                                let r = if method_name == "replaceAll" {
                                    crate::regex::js_string_replace_all_string_fn(
                                        receiver_string(),
                                        pat_str(),
                                        repl_val,
                                    )
                                } else {
                                    crate::regex::js_string_replace_string_fn(
                                        receiver_string(),
                                        pat_str(),
                                        repl_val,
                                    )
                                };
                                return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                            }
                        }
                    }
                    // Detect RegExp pattern: NaN-boxed pointer to a RegExpHeader.
                    #[cfg(feature = "regex-engine")]
                    if let Some(v) = arg_at(0) {
                        let jsv = JSValue::from_bits(v.to_bits());
                        if jsv.is_pointer() {
                            let regex_ptr = jsv.as_pointer::<crate::regex::RegExpHeader>();
                            if !regex_ptr.is_null()
                                && crate::regex::is_regex_pointer(regex_ptr as *const u8)
                            {
                                let r = if method_name == "replaceAll" {
                                    crate::regex::js_string_replace_all_regex(
                                        receiver_string(),
                                        regex_ptr,
                                        repl_str(),
                                    )
                                } else {
                                    crate::regex::js_string_replace_regex(
                                        receiver_string(),
                                        regex_ptr,
                                        repl_str(),
                                    )
                                };
                                return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
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
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                // Methods with only a codegen fast path (no native arm) — needed
                // so generic-`this` reflective calls (`String.prototype.padStart.
                // call(boxed, …)`, routed through `string_proto_thunks` after
                // coercing `this` to a string) and `(s: any).padStart(…)` dynamic
                // dispatch resolve to the runtime helper instead of the TypeError
                // catch-all. Argument coercion mirrors `lower_string_method.rs`.
                "padStart" | "padEnd" => {
                    let target_len = arg_at(0).unwrap_or(0.0);
                    // ToString(fillString) when present and not undefined; absent /
                    // undefined leaves a null ptr so the helper defaults to " ".
                    let pad = match arg_at(1) {
                        Some(v) if !JSValue::from_bits(v.to_bits()).is_undefined() => {
                            crate::builtins::js_string_coerce(v) as *const crate::StringHeader
                        }
                        _ => std::ptr::null(),
                    };
                    let s = receiver_string();
                    let r = if method_name == "padStart" {
                        crate::string::js_string_pad_start(s, target_len, pad)
                    } else {
                        crate::string::js_string_pad_end(s, target_len, pad)
                    };
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "normalize" => {
                    let form =
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits()));
                    let r = crate::string::js_string_normalize(receiver_string(), form);
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                "localeCompare" => {
                    // ToString(that) is required even for undefined ("undefined").
                    // Root it — `js_string_validate_collator_args` below may allocate.
                    let other_raw = crate::builtins::js_string_coerce(
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits())),
                    );
                    let other_h = root_scope.root_string_ptr(other_raw);
                    // `locales` (2nd) + `options` (3rd) validated for the throwing
                    // side effect of `Construct(%Collator%, «locales, options»)`.
                    if arg_at(1).is_some() || arg_at(2).is_some() {
                        let undef = f64::from_bits(JSValue::undefined().bits());
                        let loc = arg_at(1).unwrap_or(undef);
                        let opts = arg_at(2).unwrap_or(undef);
                        crate::string::js_string_validate_collator_args(loc, opts);
                    }
                    let s = receiver_string();
                    let other = other_h.get_raw_const_ptr::<crate::StringHeader>();
                    // Returns a plain f64 (-1/0/1) — NOT NaN-tagged.
                    return Some(if let Some(opts) = arg_at(2) {
                        crate::string::js_string_locale_compare_opts(s, other, opts)
                    } else {
                        crate::string::js_string_locale_compare(s, other)
                    });
                }
                "isWellFormed" => {
                    return Some(crate::string::js_string_is_well_formed(receiver_string()));
                }
                "toWellFormed" => {
                    let r = crate::string::js_string_to_well_formed(receiver_string());
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                // Annex B §B.2.2 HTML wrapper methods. No-arg tag wrappers;
                // the receiver body is never escaped.
                "big" | "blink" | "bold" | "fixed" | "italics" | "small" | "strike" | "sub"
                | "sup" => {
                    let s = receiver_string();
                    let r = match method_name {
                        "big" => crate::string::js_string_big(s),
                        "blink" => crate::string::js_string_blink(s),
                        "bold" => crate::string::js_string_bold(s),
                        "fixed" => crate::string::js_string_fixed(s),
                        "italics" => crate::string::js_string_italics(s),
                        "small" => crate::string::js_string_small(s),
                        "strike" => crate::string::js_string_strike(s),
                        "sub" => crate::string::js_string_sub(s),
                        "sup" => crate::string::js_string_sup(s),
                        _ => unreachable!(),
                    };
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                // Annex B §B.2.2 HTML wrappers that take an attribute value;
                // a missing arg coerces `undefined` -> "undefined", and `"`
                // in the value is escaped to `&quot;`.
                "anchor" | "link" | "fontcolor" | "fontsize" => {
                    let value = crate::builtins::js_string_coerce(
                        arg_at(0).unwrap_or_else(|| f64::from_bits(JSValue::undefined().bits())),
                    );
                    let value_h = root_scope.root_string_ptr(value);
                    let s = receiver_string();
                    let v = value_h.get_raw_const_ptr::<crate::StringHeader>();
                    let r = match method_name {
                        "anchor" => crate::string::js_string_anchor(s, v),
                        "link" => crate::string::js_string_link(s, v),
                        "fontcolor" => crate::string::js_string_fontcolor(s, v),
                        "fontsize" => crate::string::js_string_fontsize(s, v),
                        _ => unreachable!(),
                    };
                    return Some(f64::from_bits(JSValue::string_ptr(r).bits()));
                }
                _ => {} // not a handled string method — fall through to TypeError catch-all
            }
        }
    }

    None
}
