//! String / array / class / Map / Set / Promise / fetch / static-method
//! / instance-method dispatch — the big PropertyGet branch of
//! `lower_call`. This is by far the longest helper in this directory.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::lower_string_method;
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

use super::{
    emit_guarded_direct_method_call, emit_own_method_override_check, lower_abort_controller_call,
    lower_fetch_native_method,
};

/// Try to lower a `Call { callee: PropertyGet { .. } }` via the
/// string/array/class/Map/Set/Promise/fetch/static/instance method
/// dispatch tower. Returns `Ok(None)` when the callee shape doesn't
/// match a PropertyGet at all, OR when the PropertyGet matches but no
/// inner branch fires (caller falls through to subsequent
/// lower_call branches).
pub fn try_lower_property_get_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    let Expr::PropertyGet { object, property } = callee else {
        return Ok(None);
    };
    // Number.prototype.toFixed(decimals) — call js_number_to_fixed.
    // Receiver is any number-typed value; we don't gate on
    // is_numeric_expr because tests often call it on Any locals.
    if property == "toFixed"
        && args.len() == 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_native_module_dynamic_index(object)
    {
        let v = lower_expr(ctx, object)?;
        let dec = lower_expr(ctx, &args[0])?;
        let blk = ctx.block();
        let handle = blk.call(I64, "js_number_to_fixed", &[(DOUBLE, &v), (DOUBLE, &dec)]);
        return Ok(Some(nanbox_string_inline(blk, &handle)));
    }
    // Number.prototype.toPrecision(digits)
    if property == "toPrecision"
        && args.len() == 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_native_module_dynamic_index(object)
    {
        let v = lower_expr(ctx, object)?;
        let prec = lower_expr(ctx, &args[0])?;
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "js_number_to_precision",
            &[(DOUBLE, &v), (DOUBLE, &prec)],
        );
        return Ok(Some(nanbox_string_inline(blk, &handle)));
    }
    // Number.prototype.toExponential(decimals)
    if property == "toExponential"
        && args.len() <= 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && !is_native_module_dynamic_index(object)
    {
        let v = lower_expr(ctx, object)?;
        let dec = if args.is_empty() {
            "0.0".to_string()
        } else {
            lower_expr(ctx, &args[0])?
        };
        let blk = ctx.block();
        let handle = blk.call(
            I64,
            "js_number_to_exponential",
            &[(DOUBLE, &v), (DOUBLE, &dec)],
        );
        return Ok(Some(nanbox_string_inline(blk, &handle)));
    }
    // Buffer.prototype.toString(encoding) — handled BEFORE the radix
    // path because the encoding arg is a STRING ('utf8'/'hex'/'base64'),
    // not a number. Routing a string arg through `fptosi` produces
    // garbage and the runtime defaults to UTF-8 (the original v0.4.131
    // bug that this test pins). We dispatch via the runtime helper
    // `js_value_to_string_with_encoding` which checks BUFFER_REGISTRY
    // at runtime and falls back to `js_jsvalue_to_string` for
    // non-buffer values.
    if property == "toString"
        && args.len() == 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
        && is_string_expr(ctx, &args[0])
    {
        let has_user_to_string = receiver_class_name(ctx, object)
            .map(|cls| {
                let mut cur = Some(cls);
                while let Some(c) = cur {
                    if ctx
                        .methods
                        .contains_key(&(c.clone(), "toString".to_string()))
                    {
                        return true;
                    }
                    cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                }
                false
            })
            .unwrap_or(false);
        if !has_user_to_string {
            let v = lower_expr(ctx, object)?;
            let enc_tag_i32 = if let Expr::String(s) = &args[0] {
                let lower = s.to_ascii_lowercase();
                let tag: i32 = match lower.as_str() {
                    "utf8" | "utf-8" => 0,
                    "hex" => 1,
                    "base64" => 2,
                    "base64url" => 3,
                    "latin1" | "binary" => 4,
                    "ascii" => 5,
                    _ => 0,
                };
                tag.to_string()
            } else {
                let enc_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                blk.call(I32, "js_encoding_tag_from_value", &[(DOUBLE, &enc_box)])
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_value_to_string_with_encoding",
                &[(DOUBLE, &v), (I32, &enc_tag_i32)],
            );
            return Ok(Some(nanbox_string_inline(blk, &handle)));
        }
    }
    // Number.prototype.toString(radix) — special case where the
    // single arg is the radix (2..36). Routes through
    // js_jsvalue_to_string_radix so `(255).toString(16)` returns
    // "ff" instead of "255".
    if property == "toString"
        && args.len() == 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
    {
        // Only treat as radix call if class doesn't have toString.
        let has_user_to_string = receiver_class_name(ctx, object)
            .map(|cls| {
                let mut cur = Some(cls);
                while let Some(c) = cur {
                    if ctx
                        .methods
                        .contains_key(&(c.clone(), "toString".to_string()))
                    {
                        return true;
                    }
                    cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                }
                false
            })
            .unwrap_or(false);
        if !has_user_to_string {
            let v = lower_expr(ctx, object)?;
            let radix_d = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let radix_i32 = blk.fptosi(DOUBLE, &radix_d, I32);
            let handle = blk.call(
                I64,
                "js_jsvalue_to_string_radix",
                &[(DOUBLE, &v), (I32, &radix_i32)],
            );
            return Ok(Some(nanbox_string_inline(blk, &handle)));
        }
    }
    // Universal `.toString()` — works for any JS value via the
    // runtime's js_jsvalue_to_string dispatch (numbers print as
    // their decimal form, strings as themselves, objects as
    // [object Object], etc.). Only intercepts if NO class
    // method dispatch can win (i.e. the receiver isn't a known
    // class with its own toString) — otherwise the user's
    // override wouldn't run.
    if property == "toString"
        && args.len() <= 1
        && !is_string_expr(ctx, object)
        && !is_array_expr(ctx, object)
    {
        // Check whether the receiver class (if any) defines
        // toString itself or via inheritance.
        let has_user_to_string = receiver_class_name(ctx, object)
            .map(|cls| {
                let mut cur = Some(cls);
                while let Some(c) = cur {
                    if ctx
                        .methods
                        .contains_key(&(c.clone(), "toString".to_string()))
                    {
                        return true;
                    }
                    cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                }
                false
            })
            .unwrap_or(false);
        if !has_user_to_string {
            let v = lower_expr(ctx, object)?;
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &v)]);
            return Ok(Some(nanbox_string_inline(blk, &handle)));
        }
    }
    if is_string_expr(ctx, object) {
        return Ok(Some(lower_string_method(ctx, object, property, args)?));
    }
    // String method fallback for Any-typed receivers: when the method
    // name is a well-known string method that has no array/object
    // equivalent, route through the string dispatcher. This handles
    // the common pattern where a cross-module function returns a string
    // but the local is typed as Any (e.g., `readFileSync(path).split('\n')`).
    // Without this, .split/.charCodeAt/.charAt/etc. on Any-typed strings
    // fall through to js_native_call_method which returns [object Object].
    {
        // Only include methods that are EXCLUSIVELY string methods
        // (no array/map/set equivalent). Exclude: slice, indexOf,
        // lastIndexOf, includes, at, concat — these also exist on
        // arrays and would break when the receiver is an Any-typed
        // array. startsWith/endsWith are string-only in JS so the
        // 2-arg form (searchString, position) is also unambiguous.
        let is_string_only_method = match property.as_str() {
            "split" | "charCodeAt" | "charAt" | "trim" | "trimStart" | "trimEnd" | "substring"
            | "substr" | "toLowerCase" | "toUpperCase" | "toLocaleLowerCase"
            | "toLocaleUpperCase" | "replaceAll" | "padStart" | "padEnd" | "repeat"
            | "normalize" | "codePointAt" | "localeCompare" => true,
            // Issue #638: `replace` is also string-exclusive, but routing
            // it here unconditionally caused regressions in async dispatch
            // pathways. Only fire when args[1] is statically detectable as
            // a closure literal — that's the failing case (replace
            // callback got coerced to "[object Object]" via the runtime
            // fallback path because the string-method dispatch never
            // saw it). When args[1] is a string, the existing
            // js_native_call_method fallback handles it correctly via
            // js_string_replace_string.
            "replace" if args.len() == 2 && matches!(&args[1], Expr::Closure { .. }) => true,
            // `slice` exists on strings, arrays, buffers, and Blob-like
            // objects. Let the runtime dispatcher choose by receiver shape;
            // forcing the string path here turns Blob slices into string
            // slices for Any-typed native-module results.
            "slice" => false,
            // `indexOf` / `includes` are NOT string-forced here: an
            // Any-typed receiver may be a runtime array (e.g. a native
            // module property like `PerformanceObserver.supportedEntryTypes`),
            // and forcing the string path made `arr.includes(x)` always
            // return false (string-includes on a non-string). Falling
            // through routes both to `js_native_call_method`, which
            // dispatches on the runtime type and handles string + array
            // (with content-aware element comparison). Refs #1341.
            // startsWith / endsWith only exist on String — both 1-arg
            // and 2-arg (searchString, position) forms route here.
            "startsWith" | "endsWith" if args.len() == 1 || args.len() == 2 => true,
            "lastIndexOf" if args.len() == 1 => true,
            _ => false,
        };
        // Don't route buffer/Uint8Array methods through the string path —
        // buffers have a different header layout and their indexOf/includes
        // go through dispatch_buffer_method via js_native_call_method.
        let is_buffer = matches!(
            crate::type_analysis::static_type_of(ctx, object),
            Some(perry_types::Type::Named(ref n)) if n == "Uint8Array" || n == "Buffer"
        );
        // #1760: a dynamic native-module sub-namespace receiver
        // (`(path as any)[k]` → `path.win32`) is NOT a string, even though a
        // method like `normalize` collides with a String.prototype name.
        // Falling through here routes it to the generic `js_native_call_method`
        // dispatch (→ `dispatch_native_module_method`); forcing the string path
        // hands the namespace pointer to a string FFI and SIGSEGVs.
        if is_string_only_method
            && !is_array_expr(ctx, object)
            && !is_buffer
            && !is_native_module_dynamic_index(object)
        {
            return Ok(Some(lower_string_method(ctx, object, property, args)?));
        }
    }
    if is_array_expr(ctx, object) {
        return Ok(Some(lower_array_method(ctx, object, property, args)?));
    }

    // -------- Promise.then / .catch / .finally --------
    // Promise pointers are NaN-boxed with POINTER_TAG. We unbox
    // to get the raw i64 promise handle, then call the runtime
    // `js_promise_then(promise, on_fulfilled, on_rejected)` which
    // returns a new promise handle that we re-box with POINTER_TAG.
    //
    // `.catch(cb)` is sugar for `.then(undefined, cb)`.
    if matches!(property.as_str(), "then" | "catch" | "finally") && is_promise_expr(ctx, object) {
        match property.as_str() {
            "then" => {
                if !args.is_empty() {
                    // Fused fast path: detect `Promise.resolve(<expr>).then(cb_f, cb_e?)`
                    // and route to `js_promise_resolved_then`, which skips
                    // the intermediate Promise-#1 allocation when `<expr>`
                    // is a NaN-boxed primitive (number/bool/null/undefined/
                    // string/bigint/int32). Steady-state shape of every
                    // `await` after async-to-generator lowering — saves
                    // one Promise alloc + one TASK_QUEUE round-trip per
                    // await.
                    if let Expr::Call {
                        callee: inner_callee,
                        args: inner_args,
                        ..
                    } = object.as_ref()
                    {
                        if let Expr::PropertyGet {
                            object: inner_object,
                            property: inner_property,
                        } = inner_callee.as_ref()
                        {
                            // #1008: accept both the legacy `Promise` =
                            // GlobalGet shape and the post-#973
                            // PropertyGet { GlobalGet(0), "Promise" }
                            // shape. Without the second arm the
                            // fast path silently disengaged for
                            // every `Promise.resolve(...).then(...)`
                            // call (microtask-02..07 regression).
                            // Resolved-from-merge note: this used to live as
                            // an unresolved conflict on main; the incoming
                            // side called `is_global_constructor_expr`,
                            // which is what the rest of the file uses post
                            // #1030. Keep the richer comment from HEAD but
                            // call the same helper everything else does.
                            if inner_property == "resolve"
                                && is_global_constructor_expr(inner_object.as_ref(), "Promise")
                            {
                                let inner_value = if inner_args.is_empty() {
                                    double_literal(0.0)
                                } else {
                                    lower_expr(ctx, &inner_args[0])?
                                };
                                let on_fulfilled_box = lower_expr(ctx, &args[0])?;
                                let on_rejected_box = if args.len() >= 2 {
                                    lower_expr(ctx, &args[1])?
                                } else {
                                    "0".to_string()
                                };
                                let blk = ctx.block();
                                let on_fulfilled_handle = unbox_to_i64(blk, &on_fulfilled_box);
                                let on_rejected_handle = if args.len() >= 2 {
                                    unbox_to_i64(blk, &on_rejected_box)
                                } else {
                                    "0".to_string()
                                };
                                let new_promise = blk.call(
                                    I64,
                                    "js_promise_resolved_then",
                                    &[
                                        (DOUBLE, &inner_value),
                                        (I64, &on_fulfilled_handle),
                                        (I64, &on_rejected_handle),
                                    ],
                                );
                                return Ok(Some(nanbox_pointer_inline(blk, &new_promise)));
                            }
                        }
                    }

                    let promise_box = lower_expr(ctx, object)?;
                    let on_fulfilled_box = lower_expr(ctx, &args[0])?;
                    let on_rejected_box = if args.len() >= 2 {
                        lower_expr(ctx, &args[1])?
                    } else {
                        "0".to_string() // null → no rejection handler
                    };
                    let blk = ctx.block();
                    let promise_handle = unbox_to_i64(blk, &promise_box);
                    let on_fulfilled_handle = unbox_to_i64(blk, &on_fulfilled_box);
                    let on_rejected_i64 = if args.len() >= 2 {
                        unbox_to_i64(blk, &on_rejected_box)
                    } else {
                        "0".to_string() // null i64
                    };
                    let new_promise = blk.call(
                        I64,
                        "js_promise_then",
                        &[
                            (I64, &promise_handle),
                            (I64, &on_fulfilled_handle),
                            (I64, &on_rejected_i64),
                        ],
                    );
                    return Ok(Some(nanbox_pointer_inline(blk, &new_promise)));
                }
            }
            "catch" => {
                if !args.is_empty() {
                    let promise_box = lower_expr(ctx, object)?;
                    let on_rejected_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let promise_handle = unbox_to_i64(blk, &promise_box);
                    let on_rejected_handle = unbox_to_i64(blk, &on_rejected_box);
                    let null_i64 = "0".to_string();
                    let new_promise = blk.call(
                        I64,
                        "js_promise_then",
                        &[
                            (I64, &promise_handle),
                            (I64, &null_i64),
                            (I64, &on_rejected_handle),
                        ],
                    );
                    return Ok(Some(nanbox_pointer_inline(blk, &new_promise)));
                }
            }
            "finally" => {
                // .finally(cb) — per spec: call cb() ignoring its return value,
                // then propagate the upstream value/reason unchanged.
                // Routes through js_promise_finally which wraps cb in
                // fulfill/reject proxy closures that call cb() and then
                // return the upstream value (or re-throw the upstream reason).
                if !args.is_empty() {
                    let promise_box = lower_expr(ctx, object)?;
                    let on_finally_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let promise_handle = unbox_to_i64(blk, &promise_box);
                    let on_finally_handle = unbox_to_i64(blk, &on_finally_box);
                    let new_promise = blk.call(
                        I64,
                        "js_promise_finally",
                        &[(I64, &promise_handle), (I64, &on_finally_handle)],
                    );
                    return Ok(Some(nanbox_pointer_inline(blk, &new_promise)));
                }
            }
            _ => {}
        }
    }

    // -------- Map/Set methods on PropertyGet receivers --------
    // The HIR only folds `m.set(...)`/`m.get(...)` to MapSet/MapGet
    // when `m` is an Ident receiver (plain local). When the receiver
    // is `this.field` (class method accessing a Map-typed field),
    // the generic Call reaches here and needs an explicit dispatch
    // to the Map runtime helpers. Without this branch,
    // `this.handlers.get(event)` falls through to js_native_call_method
    // which doesn't know about Maps and returns undefined.
    if is_map_expr(ctx, object) {
        match property.as_str() {
            "set" if args.len() == 2 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let v_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void(
                    "js_map_set",
                    &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
                );
                return Ok(Some(m_box));
            }
            "get" if args.len() == 1 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                return Ok(Some(blk.call(
                    DOUBLE,
                    "js_map_get",
                    &[(I64, &m_handle), (DOUBLE, &k_box)],
                )));
            }
            "has" if args.len() == 1 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_map_has",
                    &[(I64, &m_handle), (DOUBLE, &k_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "delete" if args.len() == 1 => {
                let m_box = lower_expr(ctx, object)?;
                let k_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_map_delete",
                    &[(I64, &m_handle), (DOUBLE, &k_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "clear" if args.is_empty() => {
                let m_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void("js_map_clear", &[(I64, &m_handle)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Map iterator methods (entries / keys / values).
            // Issue #412: the HIR-level fold at expr_call.rs only
            // fires for `Expr::Ident` receivers (a plain local).
            // Receivers like `new Map(...).values()`,
            // `this.field.values()`, `obj.field.values()` come
            // through the generic call path and need codegen-time
            // dispatch — pre-fix they fell off the bottom of the
            // method-dispatch tower and silently returned
            // `undefined`. The runtime returns a real Array; we
            // NaN-box-pointer the result for downstream
            // `.length` / `forEach` / `Array.from` use.
            "entries" | "keys" | "values" if args.is_empty() => {
                let m_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                let runtime_fn = match property.as_str() {
                    "entries" => "js_map_entries",
                    "keys" => "js_map_keys",
                    "values" => "js_map_values",
                    _ => unreachable!(),
                };
                let result = blk.call(I64, runtime_fn, &[(I64, &m_handle)]);
                return Ok(Some(crate::expr::nanbox_pointer_inline_pub(blk, &result)));
            }
            _ => {}
        }
    }
    if is_set_expr(ctx, object) {
        match property.as_str() {
            "add" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let v_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_add", &[(I64, &s_handle), (DOUBLE, &v_box)]);
                return Ok(Some(s_box));
            }
            "has" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let v_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_set_has",
                    &[(I64, &s_handle), (DOUBLE, &v_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "delete" if args.len() == 1 => {
                let s_box = lower_expr(ctx, object)?;
                let v_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let i32_v = blk.call(
                    crate::types::I32,
                    "js_set_delete",
                    &[(I64, &s_handle), (DOUBLE, &v_box)],
                );
                return Ok(Some(crate::expr::i32_bool_to_nanbox(blk, &i32_v)));
            }
            "clear" if args.is_empty() => {
                let s_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_clear", &[(I64, &s_handle)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Set iterator methods. Per ECMA-262 §24.2.3.5–7,
            // `Set.prototype.values`, `.keys`, and `.entries` all
            // return iterators over the Set's elements (keys ===
            // values for Sets; entries yields [v, v] pairs).
            // Perry's `js_set_to_array` returns a real Array of
            // the Set's elements — sufficient for the common
            // `Array.from(s.values())` / `for-of s.values()` /
            // spread shapes. Pre-fix `new Set([1]).values()`
            // returned `undefined` because the HIR-level fold at
            // expr_call.rs only fires for `Expr::Ident` receivers.
            "values" | "keys" if args.is_empty() => {
                let s_box = lower_expr(ctx, object)?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                let result = blk.call(I64, "js_set_to_array", &[(I64, &s_handle)]);
                return Ok(Some(crate::expr::nanbox_pointer_inline_pub(blk, &result)));
            }
            _ => {}
        }
    }

    // -------- Map.forEach / Set.forEach --------
    // The HIR emits these as generic Call { callee: PropertyGet }
    // because it skips ArrayForEach when the receiver is Map/Set.
    // Route to the runtime forEach implementations which iterate
    // entries and call the callback via js_closure_call2.
    if property == "forEach" && !args.is_empty() {
        if is_map_expr(ctx, object) {
            let m_box = lower_expr(ctx, object)?;
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let m_handle = unbox_to_i64(blk, &m_box);
            blk.call_void("js_map_foreach", &[(I64, &m_handle), (DOUBLE, &cb_box)]);
            return Ok(Some(double_literal(0.0)));
        }
        if is_set_expr(ctx, object) {
            let s_box = lower_expr(ctx, object)?;
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let s_handle = unbox_to_i64(blk, &s_box);
            blk.call_void("js_set_foreach", &[(I64, &s_handle), (DOUBLE, &cb_box)]);
            return Ok(Some(double_literal(0.0)));
        }
        // URLSearchParams.forEach((value, key, this) => …). The HIR
        // variant `Expr::UrlSearchParamsForEach` only fires when the
        // receiver is a typed-named local; chained access (`u.searchParams
        // .forEach(...)`) and unannotated `const sp = new URLSearchParams()`
        // routes flow through this generic Call path. Route both via the
        // runtime entry so the callback gets the string `(value, key)`
        // pair instead of `(NaN, 0)` from the Array.forEach fast path.
        if is_url_search_params_expr(ctx, object) {
            let p_box = lower_expr(ctx, object)?;
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let p_handle = unbox_to_i64(blk, &p_box);
            blk.call_void(
                "js_url_search_params_for_each",
                &[(I64, &p_handle), (DOUBLE, &cb_box)],
            );
            return Ok(Some(double_literal(0.0)));
        }
    }

    // ── AbortController / AbortSignal dispatch ──
    // `new AbortController()` returns a NaN-boxed pointer
    // (refined to `Named("AbortController")`). The runtime's
    // ObjectHeader carries `signal` / `aborted` fields that the
    // generic property-get path reads. Method calls need explicit
    // interception because the class isn't in `ctx.classes`.
    if let Some(val) = lower_abort_controller_call(ctx, object, property, args)? {
        return Ok(Some(val));
    }

    // ── Chained Web Fetch dispatch ──
    // `r.headers.get(k)` — the inner `r.headers` lowered to a
    // NativeMethodCall that returns an f64 Headers handle; route
    // the outer `.get(...)` (and friends) through the Headers FFI.
    // `r.clone().status` / `.text()` / etc — the inner clone call
    // returns an f64 Response handle; route the outer call through
    // the fetch dispatch.
    //
    // `new Response(...).text()` — likewise, when the receiver is
    // a direct `Expr::New { class_name: "Response"|"Headers"|"Request" }`
    // (no intermediate let binding).
    if let Expr::NativeMethodCall {
        module: chain_mod,
        method: chain_method,
        ..
    } = object.as_ref()
    {
        // Chain `<Response>.headers.<method>(...)` where chain_method == "headers".
        if chain_mod == "fetch" && chain_method == "headers" {
            if let Some(val) =
                lower_fetch_native_method(ctx, "Headers", property.as_str(), Some(object), args)?
            {
                return Ok(Some(val));
            }
        }
        // Chain `<Response>.clone().<method>(...)` — dispatch as a
        // fetch method on the cloned handle.
        if chain_mod == "fetch" && chain_method == "clone" {
            if let Some(val) =
                lower_fetch_native_method(ctx, "fetch", property.as_str(), Some(object), args)?
            {
                return Ok(Some(val));
            }
        }
    }
    // Chain `new Response(...).text()` / `.json()` etc.
    if let Expr::New { class_name: nc, .. } = object.as_ref() {
        let fetch_dispatch = matches!(nc.as_str(), "Response" | "Headers" | "Request");
        if fetch_dispatch {
            let module = match nc.as_str() {
                "Response" => "fetch",
                "Headers" => "Headers",
                "Request" => "Request",
                _ => unreachable!(),
            };
            if let Some(val) =
                lower_fetch_native_method(ctx, module, property.as_str(), Some(object), args)?
            {
                return Ok(Some(val));
            }
        }
    }

    // Issue #687 — ClassRef receiver static-method dispatch.
    // `ClassName.method(args)` where `ClassName` lowered to
    // `Expr::ClassRef` (an INT32-NaN-boxed class id) rather than a
    // pointer to an instance. The Effect repro is Schema.ts's
    // `BigIntFromSelf.pipe(positiveBigInt(...))`, where
    // `BigIntFromSelf` is declared as
    // `class BigIntFromSelf extends make<bigint>(AST.bigIntKeyword) {}`
    // and `pipe` is a static method inherited from the anonymous
    // class returned by `make()`. Pre-fix the call fell through to
    // the dynamic-instance-dispatch tower below, which read
    // `js_object_get_class_id(0x324)` → 0 (the receiver is a class
    // id, not an instance pointer), missed every implementor case,
    // and `js_native_call_method` threw
    // `(number).pipe is not a function`.
    //
    // Resolution: when the static receiver is `Expr::ClassRef`, walk
    // the class's own static methods plus its `extends_name` chain
    // looking for `property`. If found, emit a direct call to the
    // `perry_static_<modprefix>__<class>__<method>` symbol with
    // IMPLICIT_THIS bound to the ClassRef so `pipe`'s body's
    // `this` references the class. If nothing matches (Effect's
    // BigIntFromSelf case — its parent is an unnamed CallExpr so
    // perry's `extends_name` chain is empty), fall back to
    // returning the ClassRef itself: chainable `.pipe()` calls in
    // module init then propagate the class ref forward, letting
    // Schema.ts__init advance past previously-fatal sites. The
    // returned value isn't semantically equivalent to Effect's
    // transformed schema, but it unblocks module init for the
    // #321 DoD repro.
    // Resolve the static-method receiver class through one of two
    // shapes:
    //   (a) the receiver is `Expr::ClassRef(name)` directly — the
    //       original #687 case (Effect Schema's
    //       `BigIntFromSelf.pipe(...)`); and
    //   (b) the receiver is `Expr::LocalGet(id)` where the local was
    //       initialised from `Expr::ClassRef` (or from a factory call
    //       the inliner already collapsed to ClassRef) — Effect's
    //       `const Tag = make(); Tag.staticMethod(...)`, and more
    //       generally any
    //         const C = make();
    //         C.staticMethod(...)
    //       Refs #915 (gap 2 from #899). The local→class map is the
    //       same one `lower_new`'s alias rerouting consults below.
    // Refs #915 (gap 3 / #321 follow-up): walk the receiver to
    // recognise the "static-method on a class produced by a
    // factory" pattern. Covered shapes:
    //   - `Expr::ClassRef(name)` — direct class literal.
    //   - `Expr::LocalGet(id)` whose let-init was a ClassRef (the
    //     post-#912 `const Cls = make(); Cls.foo(...)` shape).
    //   - `Expr::Call { callee: FuncRef(fid) }` where `fid` is a
    //     factory function tagged via `func_returns_class`. The
    //     HIR inliner sometimes leaves these calls in place
    //     (Effect's `Literal(value).pipe(...)`); the
    //     `func_returns_class` fixed-point pass tags Literal,
    //     makeLiteralClass, make, etc.
    //   - `Expr::Sequence` whose trailing expression itself
    //     resolves to a class. The inliner sometimes collapses
    //     `Literal(value)` to
    //     `Sequence([RegisterClassParentDynamic, ClassRef(L)])`
    //     so the call site sees the class without an outer Call.
    fn resolve_static_dispatch_cls(
        expr: &Expr,
        local_id_to_name: &std::collections::HashMap<u32, String>,
        local_class_aliases: &std::collections::HashMap<String, String>,
        func_returns_class: &std::collections::HashMap<u32, String>,
    ) -> Option<String> {
        match expr {
            Expr::ClassRef(name) => Some(name.clone()),
            // #1787: a class EXPRESSION value (`make(a) => class { ... }`,
            // lowered to `ClassExprFresh`) is a heap class object stamped
            // with the compile-time `template`'s class_id. A static-method
            // call on it (`make(a).pipe()`, the inlined factory result)
            // resolves the method through `template`'s static chain, and the
            // receiver-box selection below uses the actual object so `this`
            // carries the per-evaluation own static fields.
            Expr::ClassExprFresh { template, .. } => Some(template.clone()),
            Expr::LocalGet(id) => local_id_to_name
                .get(id)
                .and_then(|name| local_class_aliases.get(name).cloned()),
            Expr::Call { callee, .. } => match callee.as_ref() {
                Expr::FuncRef(fid) => func_returns_class.get(fid).cloned(),
                _ => None,
            },
            Expr::Sequence(exprs) => exprs.last().and_then(|e| {
                resolve_static_dispatch_cls(
                    e,
                    local_id_to_name,
                    local_class_aliases,
                    func_returns_class,
                )
            }),
            _ => None,
        }
    }
    let static_dispatch_cls: Option<String> = resolve_static_dispatch_cls(
        object,
        &ctx.local_id_to_name,
        &ctx.local_class_aliases,
        ctx.func_returns_class,
    );
    if let Some(cls_name) = static_dispatch_cls {
        // (fn_name, is_static, declared_param_count, has_rest, is_synthetic_arguments)
        let mut resolved: Option<(String, bool, usize, bool, bool)> = None;
        let mut cur = Some(cls_name.clone());
        while let Some(c) = cur {
            if let Some(class_info) = ctx.classes.get(&c) {
                let sm = class_info
                    .static_methods
                    .iter()
                    .find(|m| m.name == *property);
                if let Some(sm) = sm {
                    if let Some(fname) = ctx.methods.get(&(c.clone(), property.clone())).cloned() {
                        let declared = sm.params.len();
                        let has_rest = sm.params.last().map(|p| p.is_rest).unwrap_or(false);
                        let is_synth_args = sm
                            .params
                            .last()
                            .map(|p| p.is_rest && p.name == "arguments")
                            .unwrap_or(false);
                        resolved = Some((fname, true, declared, has_rest, is_synth_args));
                        break;
                    }
                }
            }
            cur = ctx
                .classes
                .get(&c.clone())
                .and_then(|cc| cc.extends_name.clone());
        }
        if let Some((fn_name, _is_static, declared, has_rest, is_synth_args)) = resolved {
            // Receiver-box selection (`this` inside the static body):
            //   - `ClassRef`: `lower_expr` already yields the
            //     INT32-NaN-boxed class id; `this === ClassRef`.
            //   - `Call` (factory return): `lower_expr` returns the
            //     dynamic class produced by the factory, so each
            //     `Literal(value)` / `make(ast)` call carries
            //     unique static fields (`static literals = […]`,
            //     `static ast = …`). The static body reads those
            //     through `this.<field>`, so passing the synthesized
            //     ClassRef would lose the per-call data — use the
            //     actual lowered call result instead.
            //   - Everything else (`LocalGet` after a
            //     `const Cls = make()` collapse, etc.): synthesize
            //     a fresh ClassRef NaN-box. The static body's
            //     `this.<field>` then dispatches through the
            //     ClassRef's class-keys + class-field side-table,
            //     which is the post-#912 (gap 2) shape.
            let recv_box = match object.as_ref() {
                Expr::ClassRef(_) => lower_expr(ctx, object)?,
                Expr::Call { .. } => lower_expr(ctx, object)?,
                Expr::Sequence(_) => lower_expr(ctx, object)?,
                // #1787: a class-expression value is a real heap class
                // object whose per-evaluation static fields are OWN
                // properties. Use the actual lowered object as `this` (NOT a
                // synthesized ClassRef) so `this.ast` inside the static body
                // reads this evaluation's own field rather than the shared
                // template's static-field global.
                Expr::ClassExprFresh { .. } => lower_expr(ctx, object)?,
                // #1787: `const C = make(...); C.staticMethod()`. The local
                // holds the class-expression's heap object (or, for a
                // top-level-class alias like `const F = Foo`, the same
                // INT32 ClassRef the synthesized fallback would produce).
                // Loading the actual stored value preserves the
                // per-evaluation own static fields a synthesized ClassRef
                // would discard, and is value-identical for the ClassRef
                // case — so `this.<field>` resolves correctly either way.
                Expr::LocalGet(_) => lower_expr(ctx, object)?,
                _ => {
                    // Synthesize a ClassRef NaN-box from the resolved class.
                    let cid = ctx.class_ids.get(&cls_name).copied().unwrap_or(0);
                    let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                    crate::nanbox::double_literal(f64::from_bits(bits))
                }
            };
            // Refs #915 (gap 3 / #321 follow-up): Effect's `class
            // SchemaClass { static pipe() { ... arguments ... } }`
            // factory returns an anon class whose `pipe` reads
            // `arguments.length` to dispatch. The HIR appends a
            // synthesized `arguments` rest param (#677 / #899). The
            // direct-call dispatch here previously forwarded the
            // call args 1:1 to the function whose only declared
            // parameter is the rest array — so for
            // `Cls.pipe(f1, f2)` the function got `arg0 = f1` (then
            // read .length = "function" → undefined). Mirror the
            // arg-bundling logic from the regular Call lowering
            // (lines ~720–765) so the rest slot receives a real
            // array of all call args, matching JS `arguments`
            // semantics. The non-synthetic rest path (e.g.
            // `static foo(a, ...rest)`) follows the same shape:
            // pass the first `declared-1` positional args as-is,
            // then bundle the trailing args into an Array.
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            if has_rest && is_synth_args {
                let cap = (args.len() as u32).to_string();
                let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for a in args {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
                }
                let arguments_box = nanbox_pointer_inline(ctx.block(), &current);
                lowered.push(arguments_box);
            } else if has_rest {
                let fixed_count = declared.saturating_sub(1);
                for a in args.iter().take(fixed_count) {
                    lowered.push(lower_expr(ctx, a)?);
                }
                let rest_count = args.len().saturating_sub(fixed_count);
                let cap = (rest_count as u32).to_string();
                let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for a in args.iter().skip(fixed_count) {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &current);
                lowered.push(rest_box);
            } else {
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
            }
            let prev_this =
                ctx.block()
                    .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &recv_box)]);
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let result = ctx.block().call(DOUBLE, &fn_name, &arg_slices);
            ctx.block()
                .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev_this)]);
            return Ok(Some(result));
        }
        // No static method resolved through the class's statically-visible
        // chain. #1788: a subclass of a class-expression value
        // (`class Sub extends make(...) {}`) inherits the parent's static
        // methods at RUNTIME — dispatch through the class_id parent-chain
        // walk in CLASS_STATIC_METHODS, binding `this` to the class ref so
        // `this.<field>` resolves through the subclass's static-field chain.
        // The helper returns the receiver unchanged on a genuine miss, which
        // preserves the prior "yield the class ref for a chained `.pipe()`
        // during module init" behavior for truly-absent methods.
        if matches!(object.as_ref(), Expr::ClassRef(_)) {
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Materialize the args into an entry-block `[N x double]` slot
            // (see issue #167 — alloca must live in the entry block).
            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf_reg
                ));
                (ptr_reg, n.to_string())
            };
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len = entry.byte_len.to_string();
            let blk = ctx.block();
            let name_ptr_i64 = blk.ptrtoint(&bytes_global, I64);
            return Ok(Some(blk.call(
                DOUBLE,
                "js_class_static_method_call",
                &[
                    (DOUBLE, &recv_box),
                    (I64, &name_ptr_i64),
                    (I64, &name_len),
                    (crate::types::PTR, &args_ptr),
                    (I64, &args_len),
                ],
            )));
        }
        // For LocalGet receivers that resolve to a class but the
        // method isn't a static — fall through to the normal
        // instance/dynamic dispatch tower below.
    }

    // Class instance method call. The receiver's static type is
    // `Type::Named(<class>)` for typed instances.
    //
    // Resolution strategy:
    //   1. Walk the receiver's class + parent chain to find a
    //      method named `property`. The first match (most-derived
    //      that defines the method) is the static fallback.
    //   2. Find every subclass of the receiver's class that ALSO
    //      defines the same method — those are the virtual
    //      override candidates.
    //   3. If there are no overrides, emit a direct call to the
    //      static fallback (fast path, no runtime cost).
    //   4. If there ARE overrides, emit a switch on the object's
    //      runtime class_id: each override gets its own case
    //      calling its concrete method, default falls through to
    //      the static fallback.
    // Interface / dynamic dispatch fallback: when the static
    // class is unknown OR resolves to an interface name not in
    // the class registry, BUT the property name corresponds to
    // a method defined on at least one class in the registry,
    // emit a switch on class_id over all classes that have that
    // method.
    // Skip dynamic dispatch when the receiver is GlobalGet (e.g.
    // `console.log`). GlobalGet is a module-level global object
    // (console, Math, JSON, etc.), not a class instance. Without
    // this guard, `console.log()` gets hijacked by the interface
    // dispatch tower when a user class happens to have a method
    // with the same name (like `SimpleLogger.log()`).
    let is_global = matches!(object.as_ref(), Expr::GlobalGet(_));
    // If the receiver's static type is a well-known built-in with its own
    // runtime method family (Buffer byte readers, Array, Map, Set, …),
    // don't enter the user-class dispatch tower. Otherwise an imported
    // user class that happens to declare the same method name (e.g. a
    // BufferCursor with `readUInt8`) would be enumerated as an
    // implementor and `buf.readUInt8(i)` would fall through to the
    // default 0.0 case when the Buffer's class id doesn't match any
    // tower entry.
    let is_builtin_receiver = match receiver_class_name(ctx, object) {
        Some(name) => matches!(
            name.as_str(),
            "Buffer"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int8Array"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
                | "Array"
                | "ReadonlyArray"
                | "Map"
                | "ReadonlyMap"
                | "Set"
                | "ReadonlySet"
                | "WeakMap"
                | "WeakSet"
                | "Promise"
                | "RegExp"
                | "Date"
        ),
        None => false,
    };
    let needs_dynamic_dispatch = !is_global
        && !is_builtin_receiver
        && match receiver_class_name(ctx, object) {
            None => true,
            Some(name) => !ctx.classes.contains_key(&name),
        };
    if needs_dynamic_dispatch {
        // Find all (class_id → fn_name) for `property` — including
        // INHERITED methods. Per JS spec, `subInstance.method()` for a
        // method defined on a parent dispatches to the parent's
        // implementation. perry's previous walk only added classes that
        // DIRECTLY declared `property`; subclasses that inherited the
        // method weren't represented in the dispatch tower, so the
        // icmp_eq vs class_id missed and the call fell through to the
        // runtime's js_native_call_method fallback (which returns an
        // empty object for unknown receiver class+method combos).
        // Refs #420 — drizzle's `serial("id").primaryKey()` where
        // primaryKey is on ColumnBuilder (grandparent) but the
        // receiver is a PgSerialBuilder (grandchild).
        //
        // Algorithm: walk every class C in `class_ids`. For each, walk
        // C's parent chain and find the FIRST class that has `property`
        // in `ctx.methods`. Register (C's id → that ancestor's fn_name).
        let mut implementors: Vec<(u32, String)> = Vec::new();
        let mut seen_pairs: std::collections::HashSet<(u32, String)> =
            std::collections::HashSet::new();
        for (start_cls, &start_cid) in ctx.class_ids.iter() {
            let mut cur: Option<String> = Some(start_cls.clone());
            while let Some(c) = cur {
                let key = (c.clone(), property.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    if seen_pairs.insert((start_cid, fname.clone())) {
                        implementors.push((start_cid, fname));
                    }
                    break;
                }
                cur = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
            }
        }
        if !implementors.is_empty() {
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
            lowered_args.push(recv_box.clone());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Issue #235: pad lowered_args with TAG_UNDEFINED so the callee's
            // default-param desugaring fires when the call site passed fewer
            // args than the method declares. Pre-fix the dispatch tower
            // passed exactly `args.len() + 1` doubles to a function declared
            // with N+1 doubles, leaving any param the caller skipped to be
            // read from an uninitialized arg-register slot — typically a
            // real heap pointer that hung the dispatch chain on
            // `options.session` deref.
            //
            // Take max arity across all implementors so the same arg_slices
            // works for every concrete callee. Implementations with smaller
            // arity silently ignore extra trailing args at runtime.
            let mut max_explicit_arity: usize = 0;
            for (_, fname) in &implementors {
                for ((cls, mname), reg_fname) in ctx.methods.iter() {
                    if reg_fname == fname && mname == property {
                        if let Some(&n) = ctx.method_param_counts.get(&(cls.clone(), mname.clone()))
                        {
                            if n > max_explicit_arity {
                                max_explicit_arity = n;
                            }
                        }
                        break;
                    }
                }
            }
            let target_total = max_explicit_arity + 1; // +1 for `this`
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            // Issue #672: bundle trailing args into a rest array on the
            // dynamic-dispatch path too. Mirrors the static-dispatch arm
            // below — without it, `conn.command("SET","k","v")` on a
            // `conn: any` (the @perryts/redis case) reached the callee with
            // `name="SET"`, `args="k"` and the trailing `"v"` silently
            // dropped, since the LLVM signature only declares N+1 doubles
            // and any 4th double is just discarded.
            let mut method_has_rest_dyn = false;
            let mut method_decl_count_dyn = max_explicit_arity;
            for (_, fname) in &implementors {
                for ((cls, mname), reg_fname) in ctx.methods.iter() {
                    if reg_fname == fname && mname == property {
                        let key = (cls.clone(), mname.clone());
                        if let Some(&true) = ctx.method_has_rest.get(&key) {
                            method_has_rest_dyn = true;
                            if let Some(&n) = ctx.method_param_counts.get(&key) {
                                method_decl_count_dyn = n;
                            }
                            break;
                        }
                    }
                }
                if method_has_rest_dyn {
                    break;
                }
            }
            if method_has_rest_dyn {
                let fixed_user = method_decl_count_dyn.saturating_sub(1);
                while lowered_args.len() - 1 < fixed_user {
                    lowered_args.push(undefined_lit.clone());
                }
                let split_at = 1 + fixed_user;
                let rest_count = lowered_args.len().saturating_sub(split_at);
                let cap = (rest_count as u32).to_string();
                let mut rest_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in &lowered_args[split_at..] {
                    let blk = ctx.block();
                    rest_arr = blk.call(I64, "js_array_push_f64", &[(I64, &rest_arr), (DOUBLE, v)]);
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &rest_arr);
                lowered_args.truncate(split_at);
                lowered_args.push(rest_box);
            } else {
                while lowered_args.len() < target_total {
                    lowered_args.push(undefined_lit.clone());
                }
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

            // Issue #628 followup (#620 in dynamic-dispatch shape): probe
            // own-property override BEFORE the class-id switch tower. The
            // tower hard-codes the static method body for each known
            // class id; when a user mutates `this.method = X` inside
            // a method body (hono's SmartRouter rebinds itself on first
            // call), the second call's dispatch must invoke the stored
            // override, not the original method. The static-class fast
            // path got this in v0.5.716 (#620). The dynamic-dispatch
            // path needs the parallel fix.
            let key_idx_probe = ctx.strings.intern(property);
            let probe_entry = ctx.strings.entry(key_idx_probe);
            let probe_bytes_global = format!("@{}", probe_entry.bytes_global);
            let probe_name_len_str = probe_entry.byte_len.to_string();
            let own_method_probe = ctx.block().call(
                DOUBLE,
                "js_object_get_own_field_or_undef",
                &[
                    (DOUBLE, &recv_box),
                    (crate::types::PTR, &probe_bytes_global),
                    (I64, &probe_name_len_str),
                ],
            );
            let own_bits_probe = ctx.block().bitcast_double_to_i64(&own_method_probe);
            let undef_bits_str = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
            let is_undef_probe = ctx.block().icmp_eq(I64, &own_bits_probe, &undef_bits_str);
            let probe_override_idx = ctx.new_block("idisp.override");
            let probe_dispatch_idx = ctx.new_block("idisp.dispatch");
            let probe_outer_merge_idx = ctx.new_block("idisp.outer_merge");
            let probe_override_label = ctx.block_label(probe_override_idx);
            let probe_dispatch_label = ctx.block_label(probe_dispatch_idx);
            let probe_outer_merge_label = ctx.block_label(probe_outer_merge_idx);
            ctx.block().cond_br(
                &is_undef_probe,
                &probe_dispatch_label,
                &probe_override_label,
            );

            // Override path: pack user args (skip recv at slot 0) and
            // invoke via js_native_call_value. The stored value is
            // typically an arrow function or `.bind()` closure whose
            // `this` is captured/bound, so we don't pass the receiver
            // as an extra arg — matches the static-class fast path's
            // contract.
            ctx.current_block = probe_override_idx;
            let user_arg_count_probe = lowered_args.len().saturating_sub(1);
            let (probe_args_ptr, probe_args_len_str) = if user_arg_count_probe == 0 {
                ("null".to_string(), "0".to_string())
            } else {
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count_probe);
                for (i, a_val) in lowered_args.iter().skip(1).enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, a_val, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, user_arg_count_probe, buf_reg
                ));
                (ptr_reg, user_arg_count_probe.to_string())
            };
            // Issue #632: bind IMPLICIT_THIS to the receiver around
            // the override call. The stored function may be a class
            // field assigning a non-arrow function (`class X { match
            // = match; }` — hono RegExpRouter — where the imported
            // `match` body reads `this.buildAllMatchers()`). Without
            // the bind, the body sees stale IMPLICIT_THIS and reads
            // garbage. Mirrors `lower_call.rs:2607` for the closure-
            // call fallthrough pattern (#519).
            let recv_for_this_probe = recv_box.clone();
            let prev_this_probe = ctx.block().call(
                DOUBLE,
                "js_implicit_this_set",
                &[(DOUBLE, &recv_for_this_probe)],
            );
            let v_override_probe = ctx.block().call(
                DOUBLE,
                "js_native_call_value",
                &[
                    (DOUBLE, &own_method_probe),
                    (crate::types::PTR, &probe_args_ptr),
                    (I64, &probe_args_len_str),
                ],
            );
            ctx.block().call(
                DOUBLE,
                "js_implicit_this_set",
                &[(DOUBLE, &prev_this_probe)],
            );
            let after_override_probe = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&probe_outer_merge_label);
            }

            // Dispatch path: existing class-id switch tower.
            ctx.current_block = probe_dispatch_idx;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

            // Tower of icmp+br: each implementor's case calls
            // its concrete method, default returns 0.0 (the
            // closure-call fallback would also handle this but
            // returning a sentinel is cheaper).
            let mut case_idxs: Vec<usize> = Vec::with_capacity(implementors.len());
            for (i, _) in implementors.iter().enumerate() {
                case_idxs.push(ctx.new_block(&format!("idispatch.case{}", i)));
            }
            let default_idx = ctx.new_block("idispatch.default");
            let merge_idx = ctx.new_block("idispatch.merge");
            let merge_label = ctx.block_label(merge_idx);

            for (i, (case_cid, _)) in implementors.iter().enumerate() {
                let case_label = ctx.block_label(case_idxs[i]);
                let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                if i + 1 < implementors.len() {
                    let next_idx = ctx.new_block(&format!("idispatch.test{}", i + 1));
                    let next_lbl = ctx.block_label(next_idx);
                    ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                    ctx.current_block = next_idx;
                } else {
                    let default_label = ctx.block_label(default_idx);
                    ctx.block().cond_br(&cmp, &case_label, &default_label);
                }
            }

            let mut phi_inputs: Vec<(String, String)> = Vec::new();
            for ((_, fname), &case_idx) in implementors.iter().zip(case_idxs.iter()) {
                ctx.current_block = case_idx;
                let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                let after_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v, after_label));
            }
            // Default branch: receiver's class id didn't match any user
            // class implementing `property`. Rather than returning 0.0,
            // fall through to the runtime's `js_native_call_method` so
            // same-named built-in methods (Buffer.readUInt8, Array.push,
            // Map.get, …) still reach their native dispatch. Without
            // this, a `buf.readUInt8(i)` call site ends up in the
            // default branch and returns 0, silently corrupting reads
            // any time a user class in scope happens to declare a
            // method of the same name.
            ctx.current_block = default_idx;
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len_str = entry.byte_len.to_string();
            let (fb_args_ptr, fb_args_len) = if args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                // Hoist the args-array alloca to the function entry
                // block — see issue #167 and `alloca_entry_array` doc.
                let n = args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                // skip(1) the receiver, take(n) so the issue-#235 default-arg
                // padding entries appended to lowered_args don't overflow the
                // n-sized buffer (and aren't needed for the ncm fallback path,
                // which forwards user-provided args only).
                for (i, a_val) in lowered_args.iter().skip(1).take(n).enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, a_val, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf_reg
                ));
                (ptr_reg, n.to_string())
            };
            let v_def = ctx.block().call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (crate::types::PTR, &bytes_global),
                    (I64, &name_len_str),
                    (crate::types::PTR, &fb_args_ptr),
                    (I64, &fb_args_len),
                ],
            );
            let def_label = ctx.block().label.clone();
            ctx.block().br(&merge_label);
            phi_inputs.push((v_def, def_label));

            ctx.current_block = merge_idx;
            let phi_args: Vec<(&str, &str)> = phi_inputs
                .iter()
                .map(|(v, l)| (v.as_str(), l.as_str()))
                .collect();
            let v_dispatch_phi = ctx.block().phi(DOUBLE, &phi_args);
            let after_dispatch_phi = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&probe_outer_merge_label);
            }

            // Outer merge: phi over override and dispatch values.
            ctx.current_block = probe_outer_merge_idx;
            return Ok(Some(ctx.block().phi(
                DOUBLE,
                &[
                    (v_override_probe.as_str(), after_override_probe.as_str()),
                    (v_dispatch_phi.as_str(), after_dispatch_phi.as_str()),
                ],
            )));
        }
    }

    if let Some(class_name) = receiver_class_name(ctx, object) {
        // Step 1: walk parent chain for the static method name.
        let mut static_fn: Option<String> = None;
        let mut current_class = Some(class_name.clone());
        while let Some(cur) = current_class {
            let key = (cur.clone(), property.clone());
            if let Some(fname) = ctx.methods.get(&key).cloned() {
                static_fn = Some(fname);
                break;
            }
            current_class = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
        }

        if let Some(fallback_fn) = static_fn {
            // Step 2: collect overriding subclasses. For each
            // subclass C transitively extending class_name, look
            // up which method C uses for `property` (walking C's
            // parent chain). If that resolves to a different
            // function than the static fallback, C needs an
            // explicit case in the dispatch table.
            let mut overrides: Vec<(u32, String)> = Vec::new();
            for (sub_name, &sub_id) in ctx.class_ids.iter() {
                if *sub_name == class_name {
                    continue;
                }
                // Is sub_name transitively a subclass of class_name?
                let mut parent = ctx
                    .classes
                    .get(sub_name)
                    .and_then(|c| c.extends_name.clone());
                let mut is_subclass = false;
                while let Some(p) = parent {
                    if p == class_name {
                        is_subclass = true;
                        break;
                    }
                    parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
                }
                if !is_subclass {
                    continue;
                }
                // Resolve the method for sub_name by walking its
                // own parent chain (NOT class_name's chain).
                let mut cur = Some(sub_name.clone());
                let mut sub_fn: Option<String> = None;
                while let Some(c) = cur {
                    let key = (c.clone(), property.clone());
                    if let Some(fname) = ctx.methods.get(&key).cloned() {
                        sub_fn = Some(fname);
                        break;
                    }
                    cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                }
                if let Some(sub_fn) = sub_fn {
                    if sub_fn != fallback_fn {
                        overrides.push((sub_id, sub_fn));
                    }
                }
            }

            let recv_box = lower_expr(ctx, object)?;
            let mut fallback_user_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                fallback_user_args.push(lower_expr(ctx, a)?);
            }
            let mut lowered_args: Vec<String> = Vec::with_capacity(fallback_user_args.len() + 1);
            lowered_args.push(recv_box.clone());
            lowered_args.extend(fallback_user_args.iter().cloned());
            // Issue #235: pad lowered_args with TAG_UNDEFINED so the
            // callee's default-param desugaring fires when the call site
            // passed fewer args than the method declares. Same approach
            // and reasoning as the dynamic-dispatch branch above —
            // applied here for the static-dispatch + virtual-override
            // case (receiver class IS in `ctx.classes`).
            //
            // Walk the parent chain `static_fn` was resolved through to
            // find the fallback's arity; take max across all overrides
            // so the unified arg_slices works for every concrete callee.
            let mut max_explicit_arity: usize = 0;
            let mut walk = Some(class_name.clone());
            while let Some(cur) = walk {
                let key = (cur.clone(), property.clone());
                if let Some(&n) = ctx.method_param_counts.get(&key) {
                    if n > max_explicit_arity {
                        max_explicit_arity = n;
                    }
                    break;
                }
                walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
            }
            for (sub_id, _) in &overrides {
                for (sub_name, &id) in ctx.class_ids.iter() {
                    if id == *sub_id {
                        if let Some(&n) = ctx
                            .method_param_counts
                            .get(&(sub_name.clone(), property.clone()))
                        {
                            if n > max_explicit_arity {
                                max_explicit_arity = n;
                            }
                        }
                        break;
                    }
                }
            }
            // Closes #484: bundle trailing user args into a rest
            // array when the method has a `...rest` parameter.
            // Walk the same parent chain to find has_rest. Same
            // structural shape as the freestanding-function rest
            // bundling at lower_call.rs:444 — but operates on
            // `lowered_args` after the receiver was prepended.
            let mut method_has_rest = false;
            let mut method_decl_count = max_explicit_arity;
            let mut rest_walk = Some(class_name.clone());
            while let Some(cur) = rest_walk {
                let key = (cur.clone(), property.clone());
                if let Some(&true) = ctx.method_has_rest.get(&key) {
                    method_has_rest = true;
                    method_decl_count = ctx
                        .method_param_counts
                        .get(&key)
                        .copied()
                        .unwrap_or(max_explicit_arity);
                    break;
                }
                rest_walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
            }
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            if method_has_rest {
                // user-visible fixed param count = decl - 1 (the
                // last param is the rest). lowered_args[0] is
                // `this`, [1..] are user args.
                let fixed_user = method_decl_count.saturating_sub(1);
                // Pad missing fixed args first.
                while lowered_args.len() - 1 < fixed_user {
                    lowered_args.push(undefined_lit.clone());
                }
                // Bundle remaining trailing args into a fresh
                // js_array. Index in lowered_args: 1 + fixed_user.
                let split_at = 1 + fixed_user;
                let rest_count = lowered_args.len().saturating_sub(split_at);
                let cap = (rest_count as u32).to_string();
                let mut rest_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                for v in &lowered_args[split_at..] {
                    let blk = ctx.block();
                    rest_arr = blk.call(I64, "js_array_push_f64", &[(I64, &rest_arr), (DOUBLE, v)]);
                }
                let rest_box = nanbox_pointer_inline(ctx.block(), &rest_arr);
                lowered_args.truncate(split_at);
                lowered_args.push(rest_box);
            } else {
                let target_total = max_explicit_arity + 1; // +1 for `this`
                while lowered_args.len() < target_total {
                    lowered_args.push(undefined_lit.clone());
                }
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

            if !method_has_rest {
                if let Some(guarded) = emit_guarded_direct_method_call(
                    ctx,
                    &recv_box,
                    &class_name,
                    property,
                    &fallback_fn,
                    &arg_slices,
                    &fallback_user_args,
                ) {
                    return Ok(Some(guarded));
                }
            }

            if overrides.is_empty() {
                // Issue #620: before falling through to the static method,
                // check whether the receiver has an own-property override
                // for `property` (set via `this.method = X` inside the
                // class). Hono's SmartRouter rebinds `this.match` on the
                // first call so subsequent calls go through the bound
                // fast-path closure instead of the original method.
                return Ok(Some(emit_own_method_override_check(
                    ctx,
                    &recv_box,
                    property,
                    &fallback_fn,
                    &arg_slices,
                    &lowered_args,
                )));
            }

            // Step 4: virtual dispatch via class_id switch.
            // Read class_id from the object header, then branch
            // to the right concrete method block.
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

            // Pre-create blocks: one per override + default + merge.
            let mut case_idxs: Vec<usize> = Vec::with_capacity(overrides.len());
            for (i, _) in overrides.iter().enumerate() {
                case_idxs.push(ctx.new_block(&format!("vdispatch.case{}", i)));
            }
            let default_idx = ctx.new_block("vdispatch.default");
            let merge_idx = ctx.new_block("vdispatch.merge");

            // Default → fallback. We use a tower of icmp+br rather
            // than the LLVM `switch` instruction (which the IR
            // builder doesn't expose generically) — same shape,
            // slightly more verbose.
            let mut current_label = ctx.block().label.clone();
            for (i, (case_cid, _)) in overrides.iter().enumerate() {
                let next_label = if i + 1 < overrides.len() {
                    // We'll start the next test in this same block
                    // — actually use a fresh block for the test.
                    format!("vdispatch.test{}", i + 1)
                } else {
                    ctx.block_label(default_idx)
                };
                let case_label = ctx.block_label(case_idxs[i]);
                // Make sure ctx.current_block points at the
                // current test block.
                let _ = current_label;
                let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                if i + 1 < overrides.len() {
                    // Create the next test block as a fresh block
                    // and branch into it on the false arm.
                    let next_idx = ctx.new_block(&format!("vdispatch.test{}", i + 1));
                    let next_lbl = ctx.block_label(next_idx);
                    ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                    ctx.current_block = next_idx;
                    current_label = next_lbl;
                } else {
                    ctx.block().cond_br(&cmp, &case_label, &next_label);
                }
            }

            // Each case block: call the override and branch to merge.
            let merge_label = ctx.block_label(merge_idx);
            let mut phi_inputs: Vec<(String, String)> = Vec::new();
            for ((_, fname), &case_idx) in overrides.iter().zip(case_idxs.iter()) {
                ctx.current_block = case_idx;
                let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                let after_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v, after_label));
            }

            // Default block: call the static fallback.
            ctx.current_block = default_idx;
            let v_def = ctx.block().call(DOUBLE, &fallback_fn, &arg_slices);
            let def_label = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }
            phi_inputs.push((v_def, def_label));

            // Merge: phi over all incoming case results.
            ctx.current_block = merge_idx;
            let phi_args: Vec<(&str, &str)> = phi_inputs
                .iter()
                .map(|(v, l)| (v.as_str(), l.as_str()))
                .collect();
            return Ok(Some(ctx.block().phi(DOUBLE, &phi_args)));
        }
    }
    Ok(None)
}
