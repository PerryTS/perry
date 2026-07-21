//! Logical..SetNewFromArray.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::lower_conditional::lower_logical;
use crate::nanbox::{double_literal, POINTER_MASK_I64, TAG_UNDEFINED};
use crate::type_analysis::{is_definitely_string_expr, is_numeric_expr, map_static_type_args};
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    emit_string_literal_global, i32_bool_to_nanbox, lower_expr, nanbox_pointer_inline,
    nanbox_string_inline, record_collection_number_key_fallback,
    record_collection_number_key_selected, record_collection_string_key_fallback,
    record_collection_string_key_selected, unbox_str_handle, unbox_to_i64, FnCtx,
};

fn is_static_string_key_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::String | HirType::StringLiteral(_), _])
    )
}

fn is_static_number_key_map(ctx: &FnCtx<'_>, map: &Expr) -> bool {
    matches!(
        map_static_type_args(ctx, map),
        Some([HirType::Number | HirType::Int32, _])
    )
}

fn guarded_map_number_key_delete(ctx: &mut FnCtx<'_>, map_handle: &str, key_box: &str) -> String {
    let guard_raw = ctx
        .block()
        .call(I32, "js_typed_f64_arg_guard", &[(DOUBLE, key_box)]);
    let guard = ctx.block().icmp_ne(I32, &guard_raw, "0");
    let fast_idx = ctx.new_block("map_number_key.delete.fast");
    let fallback_idx = ctx.new_block("map_number_key.delete.fallback");
    let merge_idx = ctx.new_block("map_number_key.delete.merge");
    let fast_label = ctx.block_label(fast_idx);
    let fallback_label = ctx.block_label(fallback_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&guard, &fast_label, &fallback_label);

    ctx.current_block = fast_idx;
    let key_raw = ctx
        .block()
        .call(DOUBLE, "js_typed_f64_arg_to_raw", &[(DOUBLE, key_box)]);
    let fast_value = ctx.block().call(
        I32,
        "js_map_delete_number_key",
        &[(I64, map_handle), (DOUBLE, &key_raw)],
    );
    record_collection_number_key_selected(
        ctx,
        "MapDelete",
        "collection_number_key.map_delete",
        &key_raw,
        "map",
        "number_key_helper",
        "js_map_delete_number_key",
        "key",
    );
    let after_fast = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = fallback_idx;
    let fallback_value = ctx.block().call(
        I32,
        "js_map_delete",
        &[(I64, map_handle), (DOUBLE, key_box)],
    );
    record_collection_number_key_fallback(
        ctx,
        "MapDelete",
        "collection_number_key.map_delete_generic",
        key_box,
        "map",
        "number_key_helper",
        "js_map_delete",
        "runtime_key_guard_failed",
        "key",
    );
    let after_fallback = ctx.block().label.clone();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = merge_idx;
    ctx.block().phi(
        I32,
        &[
            (fast_value.as_str(), after_fast.as_str()),
            (fallback_value.as_str(), after_fallback.as_str()),
        ],
    )
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::Logical { op, left, right } => lower_logical(ctx, *op, left, right),

        // -------- arr.filter(callback) --------
        // Mirrors ArrayMap: takes a closure header pointer, returns
        // a new array.
        Expr::ArrayFilter { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let result = blk.call(
                I64,
                "js_array_filter",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- fetch(url, { method, body, headers }) --------
        // Build a runtime headers object from the static (key, dynamic-value)
        // pairs, JSON-stringify it, and pass everything to
        // `js_fetch_with_options(url, method, body, headers_json)` which
        // returns a `*mut Promise`. The result is NaN-boxed with POINTER_TAG
        // so the rest of the await/then machinery sees a normal Promise.
        Expr::FetchWithOptions {
            url,
            method,
            body,
            headers,
            headers_dynamic,
            signal,
        } => {
            let url_box = lower_expr(ctx, url)?;
            let method_box = lower_expr(ctx, method)?;
            let body_box = lower_expr(ctx, body)?;
            // Lower `init.signal` (if any) up front so it can be stashed for
            // `js_fetch_with_options` right before the call below.
            let signal_box = match signal {
                Some(s) => Some(lower_expr(ctx, s)?),
                None => None,
            };

            // Obtain the headers as a NaN-boxed object value, then JSON-stringify
            // it below. Two cases:
            //   * `headers_dynamic` — the headers value was a variable, a spread
            //     literal, or a call (`Object.assign`/`new Headers`/`JSON.parse`).
            //     Lower it directly; `js_json_stringify` enumerates its own
            //     properties at runtime (#4932).
            //   * otherwise — statically-extracted `{ "k": v, ... }` pairs, which
            //     we build into a fresh object field-by-field.
            let headers_obj_box = if let Some(hexpr) = headers_dynamic {
                lower_expr(ctx, hexpr)?
            } else {
                // Build the headers object: js_object_alloc(0, N) followed by
                // js_object_set_field_by_name for each (interned key, value).
                let n_str = (headers.len() as u32).to_string();
                let zero_str = "0".to_string();
                let headers_handle =
                    ctx.block()
                        .call(I64, "js_object_alloc", &[(I32, &zero_str), (I32, &n_str)]);
                for (key, val_expr) in headers {
                    let key_idx = ctx.strings.intern(key);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let v_box = lower_expr(ctx, val_expr)?;
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &headers_handle), (I64, &key_raw), (DOUBLE, &v_box)],
                    );
                }
                let blk = ctx.block();
                nanbox_pointer_inline(blk, &headers_handle)
            };

            let blk = ctx.block();
            // Stringify the headers value into the flat `{name:value}` JSON that
            // `js_fetch_with_options` parses. Routed through
            // `js_fetch_headers_to_json` (not the generic `js_json_stringify`) so
            // a `Headers` instance — a fetch-band registry handle, e.g. `headers:
            // new Headers(h)` — is read from its registry instead of being
            // dereferenced as a heap pointer (the `js_json_stringify`-on-handle
            // SIGSEGV; same #5559/#5560 handle-band family).
            let headers_str = blk.call(
                I64,
                "js_fetch_headers_to_json",
                &[(DOUBLE, &headers_obj_box)],
            );

            // The runtime takes raw StringHeader pointers (i64). Unbox each
            // input string. `body` may be undefined → unbox produces 0 which
            // the runtime treats as "no body" via string_from_header().
            let url_handle = unbox_to_i64(blk, &url_box);
            let method_handle = unbox_to_i64(blk, &method_box);
            let body_handle = unbox_to_i64(blk, &body_box);
            // Stash the AbortSignal so `js_fetch_with_options` can cancel the
            // request when it aborts (`controller.abort()` / `AbortSignal.timeout`).
            if let Some(sig) = &signal_box {
                blk.call_void("js_fetch_set_pending_signal", &[(DOUBLE, sig)]);
            }
            let promise = blk.call(
                I64,
                "js_fetch_with_options",
                &[
                    (I64, &url_handle),
                    (I64, &method_handle),
                    (I64, &body_handle),
                    (I64, &headers_str),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &promise))
        }

        // -------- arr.some(callback) -> boolean --------
        // js_array_some returns a NaN-tagged TAG_TRUE/TAG_FALSE as f64,
        // so we forward it directly without conversion.
        Expr::ArraySome { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_array_some",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            ))
        }

        // -------- arr.every(callback) -> boolean --------
        Expr::ArrayEvery { array, callback } => {
            let arr_box = lower_expr(ctx, array)?;
            let cb_box = lower_expr(ctx, callback)?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_array_every",
                &[(I64, &arr_handle), (I64, &cb_handle)],
            ))
        }

        // -------- arr.join(separator?) -> string --------
        // The runtime wrapper applies Array.join separator semantics:
        // omitted/undefined means comma; every other value is ToString.
        Expr::ArrayJoin { array, separator } => {
            let arr_box = lower_expr(ctx, array)?;
            let sep_box = if let Some(sep_expr) = separator {
                lower_expr(ctx, sep_expr)?
            } else {
                double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_join_value",
                &[(I64, &arr_handle), (DOUBLE, &sep_box)],
            );
            Ok(nanbox_string_inline(blk, &result))
        }

        // -------- Array.prototype.<m>.call/apply(arrayLike, ...) (#4597) --------
        // Generic over an array-like receiver: the runtime `js_arraylike_*`
        // entry points take the *original* receiver value (NaN-boxed `f64`) so
        // they apply ToObject + LengthOfArrayLike + indexed Get/HasProperty and
        // pass the original receiver as the callback's 3rd argument. The result
        // is already a NaN-boxed JS value (number / boolean / pointer / string),
        // so it is returned directly with no re-boxing.
        Expr::ArrayLikeMethod {
            method,
            receiver,
            args,
        } => {
            let recv_box = lower_expr(ctx, receiver)?;
            let mut arg_boxes: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                arg_boxes.push(lower_expr(ctx, a)?);
            }
            let undef = || double_literal(f64::from_bits(TAG_UNDEFINED));
            let nth = |i: usize| arg_boxes.get(i).cloned();
            let blk = ctx.block();
            let result = match method.as_str() {
                // Callback iterators: (recv, callback, thisArg).
                "forEach" | "map" | "filter" | "some" | "every" | "find" | "findIndex"
                | "findLast" | "findLastIndex" => {
                    let cb = nth(0).unwrap_or_else(undef);
                    let this_arg = nth(1).unwrap_or_else(undef);
                    let fname = match method.as_str() {
                        "forEach" => "js_arraylike_forEach",
                        "map" => "js_arraylike_map",
                        "filter" => "js_arraylike_filter",
                        "some" => "js_arraylike_some",
                        "every" => "js_arraylike_every",
                        "find" => "js_arraylike_find",
                        "findIndex" => "js_arraylike_findIndex",
                        "findLast" => "js_arraylike_findLast",
                        _ => "js_arraylike_findLastIndex",
                    };
                    blk.call(
                        DOUBLE,
                        fname,
                        &[(DOUBLE, &recv_box), (DOUBLE, &cb), (DOUBLE, &this_arg)],
                    )
                }
                // Reducers: (recv, callback, has_init, init).
                "reduce" | "reduceRight" => {
                    let cb = nth(0).unwrap_or_else(undef);
                    let (has_init, init) = match nth(1) {
                        Some(i) => ("1".to_string(), i),
                        None => ("0".to_string(), undef()),
                    };
                    let fname = if method == "reduce" {
                        "js_arraylike_reduce"
                    } else {
                        "js_arraylike_reduceRight"
                    };
                    blk.call(
                        DOUBLE,
                        fname,
                        &[
                            (DOUBLE, &recv_box),
                            (DOUBLE, &cb),
                            (I32, &has_init),
                            (DOUBLE, &init),
                        ],
                    )
                }
                // Search: (recv, value, fromIndex, has_from).
                "indexOf" | "lastIndexOf" | "includes" => {
                    let value = nth(0).unwrap_or_else(undef);
                    let (has_from, from) = match nth(1) {
                        Some(f) => ("1".to_string(), f),
                        None => ("0".to_string(), undef()),
                    };
                    let fname = match method.as_str() {
                        "indexOf" => "js_arraylike_indexOf",
                        "lastIndexOf" => "js_arraylike_lastIndexOf",
                        _ => "js_arraylike_includes",
                    };
                    blk.call(
                        DOUBLE,
                        fname,
                        &[
                            (DOUBLE, &recv_box),
                            (DOUBLE, &value),
                            (DOUBLE, &from),
                            (I32, &has_from),
                        ],
                    )
                }
                // at(index): ToIntegerOrInfinity(undefined) === 0 when omitted.
                "at" => {
                    let idx = nth(0).unwrap_or_else(undef);
                    blk.call(
                        DOUBLE,
                        "js_arraylike_at",
                        &[(DOUBLE, &recv_box), (DOUBLE, &idx)],
                    )
                }
                // join(separator?): undefined separator → comma.
                "join" => {
                    let sep = nth(0).unwrap_or_else(undef);
                    blk.call(
                        DOUBLE,
                        "js_arraylike_join",
                        &[(DOUBLE, &recv_box), (DOUBLE, &sep)],
                    )
                }
                // slice(start?, end?): has-flags distinguish omitted from undefined.
                "slice" => {
                    let (has_start, start) = match nth(0) {
                        Some(s) => ("1".to_string(), s),
                        None => ("0".to_string(), undef()),
                    };
                    let (has_end, end) = match nth(1) {
                        Some(e) => ("1".to_string(), e),
                        None => ("0".to_string(), undef()),
                    };
                    blk.call(
                        DOUBLE,
                        "js_arraylike_slice",
                        &[
                            (DOUBLE, &recv_box),
                            (DOUBLE, &start),
                            (I32, &has_start),
                            (DOUBLE, &end),
                            (I32, &has_end),
                        ],
                    )
                }
                // sort(comparator?): validated + run by the runtime engine.
                "sort" => {
                    let cmp = nth(0).unwrap_or_else(undef);
                    blk.call(
                        DOUBLE,
                        "js_arraylike_sort",
                        &[(DOUBLE, &recv_box), (DOUBLE, &cmp)],
                    )
                }
                // splice(...) / concat(...): variadic — pass an alloca buffer
                // of raw NaN-boxed doubles + count (mirrors the dense
                // `js_array_concat_variadic` lowering).
                "splice" | "concat" => {
                    let n = arg_boxes.len();
                    let (buf_reg, count_str) = if n == 0 {
                        ("null".to_string(), "0".to_string())
                    } else {
                        let buf_reg = blk.next_reg();
                        blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                        for (i, val) in arg_boxes.iter().enumerate() {
                            let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                            blk.store(DOUBLE, val, &slot);
                        }
                        (buf_reg, format!("{}", n))
                    };
                    let fname = if method == "splice" {
                        "js_arraylike_splice"
                    } else {
                        "js_arraylike_concat"
                    };
                    blk.call(
                        DOUBLE,
                        fname,
                        &[(DOUBLE, &recv_box), (PTR, &buf_reg), (I32, &count_str)],
                    )
                }
                // pop() / shift(): no args, generic over a value receiver.
                "pop" | "shift" => {
                    let fname = if method == "pop" {
                        "js_arraylike_pop"
                    } else {
                        "js_arraylike_shift"
                    };
                    blk.call(DOUBLE, fname, &[(DOUBLE, &recv_box)])
                }
                // push(...) / unshift(...): variadic — pass an alloca buffer of
                // raw NaN-boxed doubles + count (mirrors splice/concat above).
                "push" | "unshift" => {
                    let n = arg_boxes.len();
                    let (buf_reg, count_str) = if n == 0 {
                        ("null".to_string(), "0".to_string())
                    } else {
                        let buf_reg = blk.next_reg();
                        blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                        for (i, val) in arg_boxes.iter().enumerate() {
                            let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                            blk.store(DOUBLE, val, &slot);
                        }
                        (buf_reg, format!("{}", n))
                    };
                    let fname = if method == "push" {
                        "js_arraylike_push"
                    } else {
                        "js_arraylike_unshift"
                    };
                    blk.call(
                        DOUBLE,
                        fname,
                        &[(DOUBLE, &recv_box), (PTR, &buf_reg), (I32, &count_str)],
                    )
                }
                other => bail!("unsupported generic array-like method '{other}'"),
            };
            Ok(result)
        }

        // -------- map.delete(key) -> boolean --------
        Expr::MapDelete { map, key } => {
            let use_string_key_map =
                is_static_string_key_map(ctx, map) && is_definitely_string_expr(ctx, key);
            let use_number_key_map = !use_string_key_map
                && is_static_number_key_map(ctx, map)
                && is_numeric_expr(ctx, key);
            let m_box = lower_expr(ctx, map)?;
            let k_box = lower_expr(ctx, key)?;
            let m_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &m_box)
            };
            let i32_v = if use_string_key_map {
                let (k_handle, i32_v) = {
                    let blk = ctx.block();
                    let k_handle = unbox_str_handle(blk, &k_box);
                    let i32_v = blk.call(
                        I32,
                        "js_map_delete_string_key",
                        &[(I64, &m_handle), (I64, &k_handle)],
                    );
                    (k_handle, i32_v)
                };
                record_collection_string_key_selected(
                    ctx,
                    "MapDelete",
                    "collection_string_key.map_delete",
                    &k_handle,
                    "map",
                    "js_map_delete_string_key",
                );
                i32_v
            } else if use_number_key_map {
                guarded_map_number_key_delete(ctx, &m_handle, &k_box)
            } else {
                let i32_v = {
                    let blk = ctx.block();
                    blk.call(I32, "js_map_delete", &[(I64, &m_handle), (DOUBLE, &k_box)])
                };
                record_collection_string_key_fallback(
                    ctx,
                    "MapDelete",
                    "collection_string_key.map_delete_generic",
                    &k_box,
                    "map",
                    "js_map_delete",
                    "receiver_or_key_not_static_string",
                );
                i32_v
            };
            let blk = ctx.block();
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- Object.keys(obj) -> string[] --------
        Expr::ObjectKeys(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            // Pass the NaN-boxed value (not an unboxed pointer) so the runtime
            // can dispatch on its tag — a string receiver yields index keys and
            // a primitive yields [], instead of crashing on a bad deref.
            let arr_handle = blk.call(I64, "js_object_keys_value", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- for (key in obj) enumeration keys -> string[] --------
        // Like ObjectKeys but nullish-safe (no throw) and walks the prototype
        // chain for inherited enumerable keys. Backs the for-in desugar.
        Expr::ForInKeys(obj) => {
            let obj_box = lower_expr(ctx, obj)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_for_in_keys_value", &[(DOUBLE, &obj_box)]);
            Ok(nanbox_pointer_inline(blk, &arr_handle))
        }

        // -------- isFinite(x) — global, coerces to Number first --------
        // The runtime's js_is_finite returns NaN-tagged TAG_TRUE/TAG_FALSE
        // (not a raw 0.0/1.0), so we return the result directly. No fcmp
        // conversion needed — TAG_TRUE is itself a NaN payload and
        // fcmp("one", NaN, 0.0) always returns false.
        Expr::IsFinite(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx.block().call(DOUBLE, "js_is_finite", &[(DOUBLE, &v)]))
        }

        // -------- Number.isFinite(x) — strict, no coercion --------
        // Per ECMA-262 §21.1.2.2, returns false for any non-Number value
        // (`"1"`, `true`, `null`, etc.) — distinct from the global
        // `isFinite` which coerces via ToNumber. Pre-fix the codegen
        // routed both forms to `js_is_finite` (the coercing variant),
        // so `Number.isFinite("1")` returned true; correct value is
        // false.
        Expr::NumberIsFinite(operand) => {
            let v = lower_expr(ctx, operand)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_number_is_finite", &[(DOUBLE, &v)]))
        }

        // -------- internal: is value === undefined OR a bare-NaN double --------
        Expr::IsUndefinedOrBareNan(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_is_undefined_or_bare_nan", &[(DOUBLE, &v)]);
            Ok(i32_bool_to_nanbox(blk, &i32_v))
        }

        // -------- Math.min(...args) --------
        // Two HIR shapes: variadic (Vec<Expr>) and spread-from-array
        // (single Expr that is an array). Both build/use an array and
        // call js_math_min_array. The variadic form materializes a
        // temporary fixed-size array via js_array_alloc + push.
        Expr::MathMin(values) => {
            if values.len() == 2 {
                let left = lower_expr(ctx, &values[0])?;
                let right = lower_expr(ctx, &values[1])?;
                let blk = ctx.block();
                return Ok(blk.call(DOUBLE, "js_math_min2", &[(DOUBLE, &left), (DOUBLE, &right)]));
            }
            let cap = (values.len() as u32).to_string();
            let arr_handle_v = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            // Push each value. push_f64 may realloc, so we thread the
            // returned pointer through.
            let mut current = arr_handle_v;
            for v_expr in values {
                let v_box = lower_expr(ctx, v_expr)?;
                let blk = ctx.block();
                current = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current), (DOUBLE, &v_box)],
                );
            }
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_math_min_array", &[(I64, &current)]))
        }
        Expr::MathMinSpread(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)]);
            Ok(blk.call(DOUBLE, "js_math_min_array", &[(I64, &arr_handle)]))
        }

        // -------- Math.max(...args) — same shape as Math.min --------
        Expr::MathMax(values) => {
            if values.len() == 2 {
                let left = lower_expr(ctx, &values[0])?;
                let right = lower_expr(ctx, &values[1])?;
                let blk = ctx.block();
                return Ok(blk.call(DOUBLE, "js_math_max2", &[(DOUBLE, &left), (DOUBLE, &right)]));
            }
            let cap = (values.len() as u32).to_string();
            let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for v_expr in values {
                let v_box = lower_expr(ctx, v_expr)?;
                let blk = ctx.block();
                current = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current), (DOUBLE, &v_box)],
                );
            }
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_math_max_array", &[(I64, &current)]))
        }
        Expr::MathMaxSpread(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let arr_handle = blk.call(I64, "js_array_like_to_array", &[(DOUBLE, &arr_box)]);
            Ok(blk.call(DOUBLE, "js_math_max_array", &[(I64, &arr_handle)]))
        }

        // -------- String(value) coercion --------
        Expr::StringCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &v)]);
            Ok(nanbox_string_inline(blk, &handle))
        }

        // -------- Object(value) coercion (#3149) --------
        // js_object_coerce takes and returns a NaN-boxed JSValue (DOUBLE):
        // nullish/primitive -> fresh {}, existing object passes through.
        Expr::ObjectCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            Ok(blk.call(DOUBLE, "js_object_coerce", &[(DOUBLE, &v)]))
        }

        // -------- Boolean(value) coercion --------
        // js_is_truthy is exactly the JS Boolean(value) coercion: it
        // returns 1 for truthy, 0 for falsy. We convert the i32 to
        // a NaN-tagged TAG_TRUE/TAG_FALSE so console.log prints
        // "true"/"false" via the runtime's NaN-tag dispatch.
        Expr::BooleanCoerce(operand) => {
            let v = lower_expr(ctx, operand)?;
            let blk = ctx.block();
            let i32_v = blk.call(I32, "js_is_truthy", &[(DOUBLE, &v)]);
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                crate::types::I1,
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }

        // -------- arr.slice(start, end?) -- new array slice --------
        Expr::ArraySlice { array, start, end } => {
            let arr_box = lower_expr(ctx, array)?;
            let start_d = lower_expr(ctx, start)?;
            let end_d = if let Some(end_expr) = end {
                lower_expr(ctx, end_expr)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            let result = blk.call(
                I64,
                "js_array_slice_values",
                &[(I64, &arr_handle), (DOUBLE, &start_d), (DOUBLE, &end_d)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- arr.shift() (HIR variant takes a LocalId) --------
        Expr::ArrayShift(array_id) => {
            let arr_box = lower_expr(ctx, &Expr::LocalGet(*array_id))?;
            let blk = ctx.block();
            let arr_handle = unbox_to_i64(blk, &arr_box);
            Ok(blk.call(DOUBLE, "js_array_shift_f64", &[(I64, &arr_handle)]))
        }

        // -------- new Set() / new Set(arr) --------
        Expr::SetNew => {
            let cap = "8".to_string();
            let handle = ctx.block().call(I64, "js_set_alloc", &[(I32, &cap)]);
            Ok(nanbox_pointer_inline(ctx.block(), &handle))
        }

        // -------- "key" in obj --------
        // js_in_operator takes two NaN-boxed doubles and returns a NaN-boxed
        // boolean (1.0/0.0 already in our ABI). Unlike the bare
        // js_object_has_property helper (used internally by Reflect.has / proxy
        // traps / `with` / rest-destructuring), the `in`-operator entry point
        // first enforces ECMA-262 13.10.1 step 5: a non-Object right operand
        // (`"x" in 5`, `... in null`, `... in Symbol()`, …) throws a TypeError.
        Expr::In { property, object } => {
            let key = lower_expr(ctx, property)?;
            let obj = lower_expr(ctx, object)?;
            Ok(ctx
                .block()
                .call(DOUBLE, "js_in_operator", &[(DOUBLE, &obj), (DOUBLE, &key)]))
        }
        Expr::PrivateBrandCheck {
            class_name,
            field_name,
            object,
        } => {
            let obj = lower_expr(ctx, object)?;
            let class_id = ctx.class_ids.get(class_name).copied().unwrap_or(0);
            let key_label = emit_string_literal_global(ctx, field_name);
            Ok(ctx.block().call(
                DOUBLE,
                "js_private_brand_check",
                &[
                    (DOUBLE, &obj),
                    (I32, &class_id.to_string()),
                    (PTR, &key_label),
                    (I32, &field_name.len().to_string()),
                ],
            ))
        }
        Expr::PrivateGuard {
            class_name,
            class_id: declaring_class_id,
            field_name,
            kind,
            op,
            object,
        } => {
            // Evaluate the receiver once, brand+kind check it, and return it
            // unchanged (or throw TypeError). The enclosing PropertyGet /
            // PropertySet / method-call lowering then operates on the result.
            let obj = lower_expr(ctx, object)?;
            // Prefer the declaring class's unique HIR id carried on the node.
            // Resolving `class_name` through `class_ids` is ambiguous: that map
            // is keyed by name (last-writer-wins), so a minified bundle that
            // reuses a class name would bind the brand to the wrong same-named
            // class and reject a legal `this.#x`. Fall back to the name lookup
            // only when the id is absent (0 = unresolved → no-op guard).
            let class_id = if *declaring_class_id != 0 {
                *declaring_class_id
            } else {
                ctx.class_ids.get(class_name).copied().unwrap_or(0)
            };
            let key_label = emit_string_literal_global(ctx, field_name);
            Ok(ctx.block().call(
                DOUBLE,
                "js_private_guard",
                &[
                    (DOUBLE, &obj),
                    (I32, &class_id.to_string()),
                    (PTR, &key_label),
                    (I32, &field_name.len().to_string()),
                    (I32, &kind.to_string()),
                    (I32, &op.to_string()),
                ],
            ))
        }

        // -------- fs.writeFileSync(path, content) --------
        // The runtime takes both args as NaN-boxed doubles directly.
        // Returns i32 (1=success); we drop the result and return 0.0
        // since the HIR-level fs.writeFileSync is void in JS.
        // -------- parseInt(string, radix?) -> number --------
        Expr::ParseInt { string, radix } => {
            let s_box = lower_expr(ctx, string)?;
            let r_d = if let Some(r_expr) = radix {
                lower_expr(ctx, r_expr)?
            } else {
                "0.0".to_string()
            };
            let blk = ctx.block();
            let s_handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &s_box)]);
            Ok(blk.call(DOUBLE, "js_parse_int", &[(I64, &s_handle), (DOUBLE, &r_d)]))
        }
        Expr::ParseFloat(string) => {
            let s_box = lower_expr(ctx, string)?;
            let blk = ctx.block();
            let s_handle = blk.call(I64, "js_string_coerce", &[(DOUBLE, &s_box)]);
            Ok(blk.call(DOUBLE, "js_parse_float", &[(I64, &s_handle)]))
        }

        // -------- RegExp literal: /pattern/flags --------
        // Constructs a RegExpHeader at compile time. Both pattern
        // and flags are interned in the StringPool so the runtime
        // sees stable handles.
        Expr::RegExp { pattern, flags } => {
            let pattern_idx = ctx.strings.intern(pattern);
            let flags_idx = ctx.strings.intern(flags);
            let pattern_global = format!("@{}", ctx.strings.entry(pattern_idx).handle_global);
            let flags_global = format!("@{}", ctx.strings.entry(flags_idx).handle_global);
            let blk = ctx.block();
            let pattern_box = blk.load(DOUBLE, &pattern_global);
            let flags_box = blk.load(DOUBLE, &flags_global);
            let pattern_handle = unbox_to_i64(blk, &pattern_box);
            let flags_handle = unbox_to_i64(blk, &flags_box);
            let result = blk.call(
                I64,
                "js_regexp_new",
                &[(I64, &pattern_handle), (I64, &flags_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // `RegExp(<dynExpr>)` / `RegExp(<dynExpr>, <dynFlagsExpr>)` /
        // `new RegExp(<non-literal>)`. Folded at HIR (lower/expr_call.rs +
        // lower/expr_new.rs) from any callsite where the pattern (or
        // flags) come in as runtime values rather than string literals.
        // Both `pattern` and `flags` are NaN-boxed strings; missing
        // flags fall back to interning an empty string at codegen so
        // `js_regexp_new` always sees a real `StringHeader*`. Followup
        // to #957 / PR #959.
        Expr::RegExpDynamic {
            pattern,
            flags,
            is_call,
        } => {
            // Route through the full ECMAScript constructor: it handles a RegExp
            // pattern (copy / flag override), an `undefined`/`null` pattern
            // (`ToString` → `""`/`"null"`), an object pattern, and ToString-
            // coerced flags (an object flags → `"[object Object]"` → SyntaxError).
            // Passing the NaN-boxed values verbatim (NOT `unbox_str_handle`,
            // which mis-reads a non-string pattern as a StringHeader → garbage).
            //
            // The function-call form `RegExp(re)` (is_call) routes through
            // `js_regexp_construct_call`, which applies the ECMA-262 22.2.4.1
            // identity shortcut (a RegExp pattern + undefined flags returns the
            // argument unchanged) before falling back to the same constructor.
            // `new RegExp(re)` keeps `js_regexp_construct` so it always copies.
            let pattern_box = lower_expr(ctx, pattern)?;
            let flags_box = if let Some(flags_expr) = flags {
                lower_expr(ctx, flags_expr)?
            } else {
                double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
            };
            let ctor = if *is_call {
                "js_regexp_construct_call"
            } else {
                "js_regexp_construct"
            };
            let blk = ctx.block();
            let result = blk.call(I64, ctor, &[(DOUBLE, &pattern_box), (DOUBLE, &flags_box)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }

        // -------- ObjectSpread literal --------
        // `{ ...a, key: val, ...b }`. The HIR carries an ordered
        // Vec<(Option<String>, Expr)>. Static props use the same
        // js_object_set_field_by_name path as `Expr::Object`. For
        // spread sources we'd need a runtime helper to copy fields
        // — for now we just allocate the object and set the static
        // props, ignoring spreads. Wrong for `...src` but unblocks
        // compilation.
        Expr::ObjectSpread { parts } => {
            // `{ ...a, x: 1, ...b, y: 2 }` — allocate an empty object,
            // then process `parts` in source order: static keys call
            // `js_object_set_field_by_name`, spreads call the runtime
            // `js_object_copy_own_fields(dst, src)` which walks the
            // source's `keys_array` and copies each field via the same
            // setter (so later parts override earlier ones, matching JS
            // semantics).
            let static_count = parts.iter().filter(|(k, _)| k.is_some()).count() as u32;
            let class_id = "0".to_string();
            let count_str = static_count.to_string();
            let obj_handle = ctx.block().call(
                I64,
                "js_object_alloc",
                &[(I32, &class_id), (I32, &count_str)],
            );
            for (key_opt, value_expr) in parts {
                if let Some(key) = key_opt {
                    // Static key:value pair.
                    let v = lower_expr(ctx, value_expr)?;
                    let key_idx = ctx.strings.intern(key);
                    let key_handle_global =
                        format!("@{}", ctx.strings.entry(key_idx).handle_global);
                    let blk = ctx.block();
                    let key_box = blk.load(DOUBLE, &key_handle_global);
                    let key_bits = blk.bitcast_double_to_i64(&key_box);
                    let key_raw = blk.and(I64, &key_bits, POINTER_MASK_I64);
                    blk.call_void(
                        "js_object_set_field_by_name",
                        &[(I64, &obj_handle), (I64, &key_raw), (DOUBLE, &v)],
                    );
                } else {
                    // `...expr` spread — copy all own fields from the
                    // source object into `obj_handle`.
                    let src_box = lower_expr(ctx, value_expr)?;
                    ctx.block().call_void(
                        "js_object_copy_own_fields",
                        &[(I64, &obj_handle), (DOUBLE, &src_box)],
                    );
                }
            }
            Ok(nanbox_pointer_inline(ctx.block(), &obj_handle))
        }

        // -------- Object.assign(target, ...sources) --------
        // Per ECMAScript spec, Object.assign mutates `target` by copying each
        // source's own enumerable string- and Symbol-keyed properties, and
        // returns `target` (same identity, class_id, and side-table state
        // preserved). The runtime helper `js_object_assign_one(t, s)` does
        // both copies for one source and returns t. We chain the calls so
        // `target` is evaluated exactly once and threaded through each source.
        // Refs #590.
        Expr::ObjectAssign { target, sources } => {
            let target_box = lower_expr(ctx, target)?;
            let mut acc = ctx.block().call(
                DOUBLE,
                "js_object_assign_validate_target",
                &[(DOUBLE, &target_box)],
            );
            // Stash target in a temp slot if there are multiple sources, so
            // each helper call uses the same SSA value (defensive: helper
            // returns target_f64 unchanged, but the chain is clearer when we
            // pass target_box explicitly each time — and side-step any LLVM
            // SSA reordering quirks). With zero sources, we still want to
            // return target itself (matching `Object.assign(t)` which is a
            // valid no-op-and-return-target form).
            if sources.is_empty() {
                return Ok(acc);
            }
            for src in sources {
                let src_box = lower_expr(ctx, src)?;
                acc = ctx.block().call(
                    DOUBLE,
                    "js_object_assign_one",
                    &[(DOUBLE, &acc), (DOUBLE, &src_box)],
                );
            }
            Ok(acc)
        }

        // -------- new Set(iter) --------
        // Fix #421 (v0.5.574): route through js_set_from_iterable so
        // string inputs (`new Set("abc")`) iterate codepoints instead of
        // segfaulting on a bad ArrayHeader cast. The runtime function
        // takes the NaN-boxed value directly and dispatches by tag.
        Expr::SetNewFromArray(arr_expr) => {
            let arr_box = lower_expr(ctx, arr_expr)?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_set_from_iterable", &[(DOUBLE, &arr_box)]);
            Ok(nanbox_pointer_inline(blk, &handle))
        }

        // -------- StaticMethodCall --------
        // `MyClass.staticMethod(args)` — look up the synthesized
        // `perry_method_<modprefix>__<class>__<method>` in the methods
        // registry and emit a direct call. Static methods don't take
        // a `this` parameter (unlike instance methods).
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
