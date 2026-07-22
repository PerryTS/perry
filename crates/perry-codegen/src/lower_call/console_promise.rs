//! Trailing `lower_call` branches:
//! - `console.log` / `console.info` / `console.warn` / …
//! - `Promise.resolve` / `.reject` / `.all` / `.race` / `.allSettled`
//!   plus `Array.fromAsync`
//! - Universal `js_native_call_method` PropertyGet dispatch
//! - Closure-call fallthrough (`recv()` where `recv` is a closure value)
//!
//! Plus the `util/types` predicate helper that bypasses the runtime
//! dispatcher for source-level `util.types.isPromise(x)` and friends after HIR
//! normalizes them to the `util/types` / `util.types` module keys.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use crate::expr::{
    emit_typed_feedback_register_site, lower_expr, nanbox_pointer_inline, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::type_analysis::{is_global_constructor_expr, receiver_class_name};
use crate::types::{DOUBLE, I32, I64, PTR};

use super::try_emit_buffer_read_intrinsic;

fn util_types_arg_is_async_function_static(ctx: &FnCtx<'_>, expr: &Expr) -> Option<bool> {
    match expr {
        Expr::FuncRef(fid) => Some(ctx.local_async_funcs.contains(fid)),
        Expr::Closure { is_async, .. } => Some(*is_async),
        Expr::LocalGet(id) => match ctx.local_types.get(id) {
            Some(HirType::Function(ft)) => Some(ft.is_async),
            _ => None,
        },
        _ => None,
    }
}

fn nanbox_bool_literal(value: bool) -> String {
    double_literal(f64::from_bits(if value {
        crate::nanbox::TAG_TRUE
    } else {
        crate::nanbox::TAG_FALSE
    }))
}

fn lower_util_types_predicate_arg(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::NativeMethodCall {
        module,
        class_name,
        method,
        object,
        args,
        ..
    } = expr
    else {
        return Ok(None);
    };
    let is_direct_util_types_module =
        matches!(module.as_str(), "util/types" | "util.types") && class_name.is_none();
    if !is_direct_util_types_module || object.is_some() {
        return Ok(None);
    }
    if method == "isAsyncFunction" {
        if let Some(is_async) = args
            .first()
            .and_then(|arg| util_types_arg_is_async_function_static(ctx, arg))
        {
            return Ok(Some(nanbox_bool_literal(is_async)));
        }
        let value = if let Some(first) = args.first() {
            lower_expr(ctx, first)?
        } else {
            double_literal(0.0)
        };
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_util_types_is_async_function",
            &[(DOUBLE, &value)],
        )));
    }
    let Some(runtime) = (match method.as_str() {
        "isArgumentsObject" => Some("js_util_types_is_arguments_object"),
        "isPromise" => Some("js_util_types_is_promise"),
        "isBigIntObject" => Some("js_util_types_is_big_int_object"),
        "isArrayBuffer" => Some("js_util_types_is_array_buffer"),
        "isSharedArrayBuffer" => Some("js_util_types_is_shared_array_buffer"),
        "isAnyArrayBuffer" => Some("js_util_types_is_any_array_buffer"),
        "isArrayBufferView" => Some("js_util_types_is_array_buffer_view"),
        "isDataView" => Some("js_util_types_is_data_view"),
        "isTypedArray" => Some("js_util_types_is_typed_array"),
        "isUint8Array" => Some("js_util_types_is_uint8_array"),
        "isInt8Array" => Some("js_util_types_is_int8_array"),
        "isInt16Array" => Some("js_util_types_is_int16_array"),
        "isUint16Array" => Some("js_util_types_is_uint16_array"),
        "isInt32Array" => Some("js_util_types_is_int32_array"),
        "isUint32Array" => Some("js_util_types_is_uint32_array"),
        "isFloat16Array" => Some("js_util_types_is_float16_array"),
        "isFloat32Array" => Some("js_util_types_is_float32_array"),
        "isFloat64Array" => Some("js_util_types_is_float64_array"),
        "isUint8ClampedArray" => Some("js_util_types_is_uint8_clamped_array"),
        "isBigInt64Array" => Some("js_util_types_is_big_int64_array"),
        "isBigUint64Array" => Some("js_util_types_is_big_uint64_array"),
        "isMap" => Some("js_util_types_is_map"),
        "isMapIterator" => Some("js_util_types_is_map_iterator"),
        "isProxy" => Some("js_util_types_is_proxy"),
        "isExternal" => Some("js_util_types_is_external"),
        "isModuleNamespaceObject" => Some("js_util_types_is_module_namespace_object"),
        "isSet" => Some("js_util_types_is_set"),
        "isSetIterator" => Some("js_util_types_is_set_iterator"),
        "isWeakMap" => Some("js_util_types_is_weak_map"),
        "isWeakSet" => Some("js_util_types_is_weak_set"),
        "isDate" => Some("js_util_types_is_date"),
        "isRegExp" => Some("js_util_types_is_reg_exp"),
        "isAsyncFunction" => Some("js_util_types_is_async_function"),
        "isGeneratorFunction" => Some("js_util_types_is_generator_function"),
        "isGeneratorObject" => Some("js_util_types_is_generator_object"),
        "isNativeError" => Some("js_util_types_is_native_error"),
        "isKeyObject" => Some("js_util_types_is_key_object"),
        "isCryptoKey" => Some("js_util_types_is_crypto_key"),
        "isNumberObject" => Some("js_util_types_is_number_object"),
        "isStringObject" => Some("js_util_types_is_string_object"),
        "isBooleanObject" => Some("js_util_types_is_boolean_object"),
        "isSymbolObject" => Some("js_util_types_is_symbol_object"),
        "isBoxedPrimitive" => Some("js_util_types_is_boxed_primitive"),
        _ => None,
    }) else {
        return Ok(None);
    };
    let value = if let Some(first) = args.first() {
        lower_expr(ctx, first)?
    } else {
        double_literal(0.0)
    };
    Ok(Some(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &value)])))
}

pub fn try_lower_console_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // console.log(<args...>) sink.
    //
    // JS spec: console.log can take any number of args, separated by
    // single spaces. We approximate by emitting a separate dispatch
    // call per arg with a literal " " in between, then a final "\n".
    // The runtime functions take a NaN-boxed double and print it
    // followed by a single trailing space (for the inter-arg form)
    // or newline (for the final/single-arg form). For now we use the
    // existing js_console_log_dynamic for every arg — the runtime
    // already adds a newline, so multi-arg console.log will be
    // separated by newlines instead of spaces. Spec-compliant
    // separator handling lives in a future Phase I tweak.
    if let Expr::PropertyGet {
        object, property, ..
    } = callee
    {
        if matches!(object.as_ref(), Expr::GlobalGet(_))
            && matches!(
                property.as_str(),
                "log"
                    | "info"
                    | "warn"
                    | "error"
                    | "debug"
                    | "dir"
                    | "table"
                    | "trace"
                    | "group"
                    | "groupEnd"
                    | "groupCollapsed"
                    | "time"
                    | "timeEnd"
                    | "timeLog"
                    | "count"
                    | "countReset"
                    | "clear"
                    | "assert"
            )
        {
            // Catch-all for the entire console.* surface. Most of
            // them are best-effort: we route the args through
            // js_console_log_dynamic so the user at least sees the
            // values, then return undefined-as-double. Spec-compliant
            // dispatch (separate stderr for warn/error, dir's depth
            // option, table's tabular layout) is a future improvement.
            // Zero-arg console.* calls — handle the truly nullary
            // methods (groupEnd, clear) and the dataless variants of
            // log/info/warn/error/debug (which print nothing). Methods
            // with meaningful zero-arg semantics (count, countReset,
            // time, timeEnd, timeLog with the implicit "default" label)
            // intentionally fall through to the dedicated handler below.
            if args.is_empty() {
                match property.as_str() {
                    "groupEnd" => {
                        ctx.block().call_void("js_console_group_end", &[]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    "clear" => {
                        ctx.block().call_void("js_console_clear", &[]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    "group" | "groupCollapsed" => {
                        ctx.block().call_void("js_console_group_begin", &[]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    "count" | "countReset" | "time" | "timeEnd" | "timeLog" => {
                        // Fall through to the dedicated handler below
                        // which calls the runtime with the implicit
                        // "default" label.
                    }
                    "log" | "info" | "debug" => {
                        // Issue #557: zero-arg console.log()/info()/debug()
                        // emits a newline to stdout (matches Node/bun). The
                        // *_spread runtime fns already print just `\n` when
                        // their arg is null, so pass i64 0 directly.
                        ctx.block()
                            .call_void("js_console_log_spread", &[(I64, "0")]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    "warn" => {
                        ctx.block()
                            .call_void("js_console_warn_spread", &[(I64, "0")]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    "error" => {
                        ctx.block()
                            .call_void("js_console_error_spread", &[(I64, "0")]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    "trace" => {
                        let val = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        ctx.block().call_void("js_console_trace", &[(DOUBLE, &val)]);
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                    _ => {
                        // Other zero-arg console.* methods (dir, assert,
                        // etc.) — print nothing.
                        return Ok(Some(double_literal(f64::from_bits(
                            crate::nanbox::TAG_UNDEFINED,
                        ))));
                    }
                }
            }
            // console.group / groupCollapsed with a label — push
            // indent level and print the label.
            if matches!(property.as_str(), "group" | "groupCollapsed") {
                for a in args {
                    let v = lower_expr(ctx, a)?;
                    ctx.block()
                        .call_void("js_console_log_dynamic", &[(DOUBLE, &v)]);
                }
                ctx.block().call_void("js_console_group_begin", &[]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // console.trace([msg]) — `js_console_trace` formats the
            // optional message and emits a native backtrace to stderr
            // (issue #20).
            if property == "trace" {
                if args.is_empty() {
                    let val = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    ctx.block().call_void("js_console_trace", &[(DOUBLE, &val)]);
                } else {
                    let cap = (args.len() as u32).to_string();
                    let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for arg in args.iter() {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        current_arr = blk.call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &v)],
                        );
                    }
                    ctx.block()
                        .call_void("js_console_trace_spread", &[(I64, &current_arr)]);
                }
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // console.table(data[, properties]) — dedicated table renderer.
            if property == "table" && (args.len() == 1 || args.len() == 2) {
                let v = lower_expr(ctx, &args[0])?;
                if args.len() == 2 {
                    let props = lower_expr(ctx, &args[1])?;
                    ctx.block().call_void(
                        "js_console_table_with_properties",
                        &[(DOUBLE, &v), (DOUBLE, &props)],
                    );
                } else {
                    ctx.block().call_void("js_console_table", &[(DOUBLE, &v)]);
                }
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // console.time(label) / timeEnd(label) / timeLog(label) —
            // dedicated timer functions that track per-label Instants
            // in a thread-local HashMap. Without this dispatch the
            // label got routed through js_console_log_dynamic and just
            // printed the string, losing the elapsed-time output.
            if matches!(
                property.as_str(),
                "time" | "timeEnd" | "timeLog" | "count" | "countReset"
            ) && !args.is_empty()
            {
                let v = lower_expr(ctx, &args[0])?;
                if property == "timeLog" && args.len() > 1 {
                    let cap = ((args.len() - 1) as u32).to_string();
                    let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for arg in args.iter().skip(1) {
                        let extra = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        current_arr = blk.call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &extra)],
                        );
                    }
                    ctx.block().call_void(
                        "js_console_time_log_spread",
                        &[(DOUBLE, &v), (I64, &current_arr)],
                    );
                    return Ok(Some(double_literal(f64::from_bits(
                        crate::nanbox::TAG_UNDEFINED,
                    ))));
                }
                let runtime_fn = match property.as_str() {
                    "time" => "js_console_time_value",
                    "timeEnd" => "js_console_time_end_value",
                    "timeLog" => "js_console_time_log_value",
                    "count" => "js_console_count_value",
                    "countReset" => "js_console_count_reset_value",
                    _ => unreachable!(),
                };
                ctx.block().call_void(runtime_fn, &[(DOUBLE, &v)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Zero-arg time* / count* use the default label "default".
            if matches!(
                property.as_str(),
                "time" | "timeEnd" | "timeLog" | "count" | "countReset"
            ) && args.is_empty()
            {
                let sp_idx = ctx.strings.intern("default");
                let sp_global = format!("@{}", ctx.strings.entry(sp_idx).handle_global);
                let blk = ctx.block();
                let sp_box = blk.load(DOUBLE, &sp_global);
                let handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &sp_box)]);
                let runtime_fn = match property.as_str() {
                    "time" => "js_console_time",
                    "timeEnd" => "js_console_time_end",
                    "timeLog" => "js_console_time_log",
                    "count" => "js_console_count",
                    "countReset" => "js_console_count_reset",
                    _ => unreachable!(),
                };
                blk.call_void(runtime_fn, &[(I64, &handle)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // console.assert(cond[, ...messages]) — runtime helper
            // checks the condition and only prints "Assertion failed: msg"
            // when cond is falsy. Without this dedicated dispatch, the call
            // fell through to the multi-arg console.log path which
            // printed both cond and messages unconditionally ("true should
            // not appear" / "false assertion failed message").
            //
            // Two shapes:
            //   1. 0–1 message args → js_console_assert(cond, msg_ptr)
            //   2. 2+ message args  → bundle into array, call
            //      js_console_assert_spread(cond, arr_ptr) which formats
            //      each element with format_jsvalue and joins with spaces.
            if property == "assert" {
                let cond_v = if args.is_empty() {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                } else {
                    lower_expr(ctx, &args[0])?
                };
                if args.len() <= 1 {
                    ctx.block()
                        .call_void("js_console_assert", &[(DOUBLE, &cond_v), (I64, "0")]);
                } else {
                    // Multi-arg messages: bundle args[1..] into a heap
                    // array and call the spread variant.
                    let cap = ((args.len() - 1) as u32).to_string();
                    let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for arg in args.iter().skip(1) {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        current_arr = blk.call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &v)],
                        );
                    }
                    ctx.block().call_void(
                        "js_console_assert_spread",
                        &[(DOUBLE, &cond_v), (I64, &current_arr)],
                    );
                }
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // console.dir(obj[, options]) — always routes through
            // `js_console_dir_with_options` so the runtime can apply Node's
            // dir-specific defaults (depth=2 #1199, showHidden=false #1200,
            // customInspect=false #1201). Missing `options` arg becomes
            // `undefined` and the option decoders fall back to their
            // Node-compatible defaults.
            if property == "dir" && !args.is_empty() {
                let v = lower_expr(ctx, &args[0])?;
                let opts = if args.len() >= 2 {
                    lower_expr(ctx, &args[1])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                ctx.block().call_void(
                    "js_console_dir_with_options",
                    &[(DOUBLE, &v), (DOUBLE, &opts)],
                );
                for a in args.iter().skip(2) {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Single-arg fast path: just print directly. Pre-fix #345 this
            // ignored the `property` and always called `js_console_log_*`,
            // which collapsed `console.error("x")` and `console.warn("x")`
            // onto stdout. Dispatch on property so each console method
            // routes to its matching runtime fn (and stream).
            if args.len() == 1 {
                let arg = &args[0];
                let v = if let Some(v) = lower_util_types_predicate_arg(ctx, arg)? {
                    v
                } else {
                    lower_expr(ctx, arg)?
                };
                let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, "1")]);
                current_arr = ctx.block().call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current_arr), (DOUBLE, &v)],
                );
                let runtime_fn = match property.as_str() {
                    "info" => "js_console_info_spread",
                    "debug" => "js_console_debug_spread",
                    "warn" => "js_console_warn_spread",
                    "error" => "js_console_error_spread",
                    _ => "js_console_log_spread",
                };
                ctx.block().call_void(runtime_fn, &[(I64, &current_arr)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            // Multi-arg: bundle all args into a heap array and call
            // js_console_log_spread, which uses the runtime's
            // format_jsvalue (Node-style util.inspect output for
            // objects/arrays). This is more accurate than
            // js_jsvalue_to_string which only does the JS toString
            // protocol (returns "[object Object]" for plain objects).
            let cap = (args.len() as u32).to_string();
            let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for arg in args.iter() {
                let v = if let Some(v) = lower_util_types_predicate_arg(ctx, arg)? {
                    v
                } else {
                    lower_expr(ctx, arg)?
                };
                let blk = ctx.block();
                current_arr = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current_arr), (DOUBLE, &v)],
                );
            }
            let runtime_fn = match property.as_str() {
                "info" => "js_console_info_spread",
                "debug" => "js_console_debug_spread",
                "warn" => "js_console_warn_spread",
                "error" => "js_console_error_spread",
                _ => "js_console_log_spread",
            };
            ctx.block().call_void(runtime_fn, &[(I64, &current_arr)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
    }
    Ok(None)
}

pub fn try_lower_promise_static_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // -------- Promise.resolve / reject / all / race / allSettled --------
    //
    // The HIR doesn't have dedicated PromiseResolve/Reject variants. Depending
    // on the lowering path they appear either as a bare GlobalGet receiver or
    // as `globalThis.Promise.<method>`.
    if let Expr::PropertyGet {
        object, property, ..
    } = callee
    {
        if is_global_constructor_expr(object, "Promise") {
            match property.as_str() {
                "resolve" => {
                    let value = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &value)]);
                    return Ok(Some(nanbox_pointer_inline(blk, &handle)));
                }
                "reject" => {
                    let reason = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_rejected", &[(DOUBLE, &reason)]);
                    return Ok(Some(nanbox_pointer_inline(blk, &handle)));
                }
                "all" | "race" | "allSettled" | "any" => {
                    // Issue #2822: the combinators accept any iterable and must
                    // reject with `TypeError` for non-iterable / omitted input
                    // (`undefined` is not iterable). Pass the boxed argument
                    // value to the `*_iterable` runtime entry points, which
                    // coerce iterables to an array and produce the rejected
                    // Promise otherwise. A missing argument lowers to
                    // `undefined` so the runtime rejects it just like Node.
                    let value = if args.is_empty() {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let runtime_fn = match property.as_str() {
                        "all" => "js_promise_all_iterable",
                        "race" => "js_promise_race_iterable",
                        "any" => "js_promise_any_iterable",
                        _ => "js_promise_all_settled_iterable",
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, runtime_fn, &[(DOUBLE, &value)]);
                    return Ok(Some(nanbox_pointer_inline(blk, &handle)));
                }
                "withResolvers" => {
                    // Promise.withResolvers<T>() returns { promise, resolve, reject }.
                    // We create a pending promise and return an object with
                    // the promise + resolve/reject closures.
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_with_resolvers", &[]);
                    return Ok(Some(nanbox_pointer_inline(blk, &handle)));
                }
                "try" => {
                    let callback = if args.is_empty() {
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let extra_count = args.len().saturating_sub(1);
                    let mut current_arr =
                        ctx.block()
                            .call(I64, "js_array_alloc", &[(I32, &extra_count.to_string())]);
                    for arg in args.iter().skip(1) {
                        let value = lower_expr(ctx, arg)?;
                        current_arr = ctx.block().call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &value)],
                        );
                    }
                    let blk = ctx.block();
                    let handle = blk.call(
                        I64,
                        "js_promise_try",
                        &[(DOUBLE, &callback), (I64, &current_arr)],
                    );
                    return Ok(Some(nanbox_pointer_inline(blk, &handle)));
                }
                _ => {}
            }
        }
        // `Array.fromAsync(input, mapFn?, thisArg?)` — Node 22+ static method.
        if is_global_constructor_expr(object, "Array") && property == "fromAsync" {
            let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let input = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                undefined.clone()
            };
            let map_fn = if let Some(arg) = args.get(1) {
                lower_expr(ctx, arg)?
            } else {
                undefined.clone()
            };
            let this_arg = if let Some(arg) = args.get(2) {
                lower_expr(ctx, arg)?
            } else {
                undefined
            };
            let blk = ctx.block();
            return Ok(Some(blk.call(
                DOUBLE,
                "js_array_from_async",
                &[(DOUBLE, &input), (DOUBLE, &map_fn), (DOUBLE, &this_arg)],
            )));
        }
    }
    Ok(None)
}

pub fn try_lower_native_method_str_dispatch(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // #5247: capture this call's source byte offset before any argument
    // (possibly a nested call that overwrites the shared pending offset) is
    // lowered, so the dynamic dispatch below can emit `js_set_call_location`
    // with the OUTER call's location right before the throw-capable dispatch.
    let call_byte_offset = ctx.strings.pending_call_offset();
    // -------- PropertyGet method dispatch via js_native_call_method --------
    //
    // For `recv.method(args)` where the static dispatch above didn't fire
    // and the receiver isn't a known class instance, route through the
    // runtime's universal `js_native_call_method` dispatcher. This is the
    // path that catches Map/Set/RegExp methods on plain object fields
    // (e.g. `wrap.m.get(k)` where `wrap: { m: Map }`) — the runtime
    // detects the registry and dispatches to `js_map_get` etc. directly.
    //
    // The signature is `js_native_call_method(obj: f64, name_ptr: ptr,
    // name_len: i64, args_ptr: ptr, args_len: i64) -> f64`. We pass the
    // method name as a raw rodata byte pointer (the StringPool already
    // emits the bytes as `[N+1 x i8]` for every interned string), and
    // materialize the args into a stack `[N x double]` slot.
    if let Expr::PropertyGet {
        object, property, ..
    } = callee
    {
        if is_message_port_closure_method(object, property) {
            return Ok(None);
        }
        // Skip when the receiver is a global module access (e.g. `console.log`,
        // `JSON.parse`) — those are handled by the spread/closure paths above
        // or have dedicated lowerings. Skip when the receiver is a known class
        // instance — those have static method dispatch handled earlier.
        //
        // Exception: `Uint8Array`/`Buffer` typed receivers must NOT be skipped.
        // They aren't real classes (no vtable) — the runtime's
        // `js_native_call_method` detects them via `is_registered_buffer` and
        // routes through `dispatch_buffer_method` which handles the full
        // Node-style numeric read/write/swap/indexOf method family.
        //
        // Issue #510: also skip `NativeModuleRef` receivers (e.g. unknown
        // `fs.*` / `crypto.*` calls that fall through their dedicated arms).
        // `NativeModuleRef` lowers to literal `0.0`, which the runtime
        // catch-all would treat as a primitive (`number`) and throw on. The
        // pre-#510 behavior was a silent NULL_OBJECT_BYTES fallback —
        // matching that here keeps "unsupported native module method" cases
        // returning undefined instead of throwing. (Throwing would be more
        // helpful but requires per-module unimplemented-API detection at the
        // codegen site, tracked as part of the unimplemented-API plan in
        // #463.)
        let class_name_opt = receiver_class_name(ctx, object);
        let is_buffer_class = matches!(
            class_name_opt.as_deref(),
            Some("Uint8Array") | Some("Buffer") | Some("Uint8ClampedArray")
        );
        // Issue #392 followup: when the receiver's static class name is known
        // but the class is NOT in `ctx.classes` (the canonical case is a
        // type-only `import type { Changeset } from "./changeset"` which
        // strips the module from `hir.imports` and produces no entry in
        // `imported_classes` for this consumer module — see
        // crates/perry/src/commands/compile.rs::is_unresolved_name where the
        // class is considered "resolved" because it's in
        // `all_program_type_names`, which short-circuits the
        // `references_interface` full-visibility fallback), the static
        // dispatch tower above can't find a method entry and would fall
        // through to `js_closure_call<N>` against `obj.<method>` read as a
        // closure-valued property — which silently no-ops on Map/Set field
        // mutations like `this.adds.set(k, v)` inside the cross-module
        // method. Route through `js_native_call_method` instead so the
        // runtime's `CLASS_VTABLE_REGISTRY` (populated by v0.5.464's
        // `js_register_class_method` calls in `emit_string_pool`) dispatches
        // to the real `perry_method_<modprefix>__<class>__<method>`.
        let class_unknown_to_codegen = class_name_opt
            .as_ref()
            .is_some_and(|n| !ctx.classes.contains_key(n));
        // Well-known `Object.prototype` / `Function.prototype` methods —
        // any user class instance can have them invoked via the
        // prototype chain. Pre-fix the static class-dispatch path
        // skipped `js_native_call_method` entirely when the receiver's
        // class WAS known to codegen, which made `({ k: null }).propertyIsEnumerable("k")`
        // (ramda's `keys.js` IIFE) fall into the closure-call fallback
        // that read `propertyIsEnumerable` as a property value
        // (returning `undefined`) and threw `value is not a function`.
        let is_well_known_proto_method = matches!(
            property.as_str(),
            "hasOwnProperty"
                | "propertyIsEnumerable"
                | "isPrototypeOf"
                | "toLocaleString"
                | "valueOf"
                // Annex B §B.2.2 Object.prototype accessor helpers — handled
                // by `js_native_call_method`; the static class-dispatch tower
                // would read them as a non-callable property and throw
                // (test262 elements/private-getter-is-not-a-own-property).
                | "__lookupGetter__"
                | "__lookupSetter__"
                | "__defineGetter__"
                | "__defineSetter__"
                // #4795: the `using`-declaration disposability validator is not
                // a class method — it must reach the runtime `js_native_call_method`
                // handler (which checks symbol keys + the class vtable) rather
                // than the static class-dispatch tower, which would treat the
                // missing method as a non-callable property read and throw.
                | "__perry_using_check__"
        );
        // A `class X extends Map | Set` instance's collection methods
        // (`has`/`get`/`set`/`delete`/`clear`/`forEach`/`keys`/`values`/
        // `entries` and the Set composition methods) are NOT class methods —
        // they live on the hidden runtime backing installed by
        // `js_map_set_subclass_init`. The static class-dispatch tower would read
        // them as a non-callable property and throw "value is not a function",
        // so route them through `js_native_call_method` (whose `dispatch_map_set`
        // redirects onto the backing collection). Mirrors the
        // `is_well_known_proto_method` carve-out.
        // Only Map/Set subclasses get a runtime backing installed at `super()`
        // (via `js_map_set_subclass_init`); WeakMap/WeakSet subclasses have NO
        // backing, so leave their method calls on the NORMAL class-dispatch path
        // instead of suppressing it toward a non-existent backing (which would
        // also shadow a user's own override on a WeakMap/WeakSet subclass).
        let is_collection_subclass_method = class_name_opt
            .as_deref()
            .and_then(|n| class_builtin_collection_kind(ctx, n))
            .filter(|kind| matches!(*kind, "Map" | "Set"))
            .is_some_and(|kind| is_collection_method_for_kind(kind, property.as_str()));
        // A `class X extends Array` instance's inherited Array methods
        // (`push`/`map`/`join`/…) are NOT class methods — they live on the
        // runtime's spec-generic array-like engine over the plain `ObjectHeader`
        // instance. The static class-dispatch tower above finds no method entry
        // (Array is not a user class) and returns None, so without this carve-out
        // the call falls into the closure-call fallthrough, which reads the
        // method as a non-callable property and throws "value is not a function".
        // Route it through `js_native_call_method` (whose relaxed plain-object
        // mutator guard + Array-subclass read arm handle it). A user-defined
        // override of the same name resolves earlier via the static tower, so it
        // never reaches here. Mirrors the `is_collection_subclass_method`
        // carve-out for Map/Set subclasses.
        let is_array_subclass_method = class_name_opt
            .as_deref()
            .is_some_and(|n| class_extends_builtin_array(ctx, n))
            && is_array_like_method(property.as_str());
        // A `class X extends URLSearchParams` instance's methods (`get`/`has`/
        // `set`/…) are NOT class methods — they live on the hidden native
        // backing installed by `js_url_search_params_subclass_init` at
        // `super()`. The static class-dispatch tower would read them as a
        // non-callable property and throw "value is not a function", so route
        // them through `js_native_call_method` (whose `dispatch_url_search_params`
        // redirects onto the backing). Mirrors `is_collection_subclass_method`.
        let is_url_search_params_subclass_method = class_name_opt
            .as_deref()
            .is_some_and(|n| crate::type_analysis::class_name_extends_url_search_params(ctx, n))
            && is_url_search_params_method(property.as_str());
        let skip_native = matches!(object.as_ref(), Expr::GlobalGet(_))
            || matches!(object.as_ref(), Expr::NativeModuleRef(_))
            || (class_name_opt.is_some()
                && !is_buffer_class
                && !class_unknown_to_codegen
                && !is_well_known_proto_method
                && !is_collection_subclass_method
                && !is_array_subclass_method
                && !is_url_search_params_subclass_method);
        if !skip_native {
            // Issue #92 fast path: intrinsify Buffer numeric reads
            // (`buf.readInt32BE(off)` etc.) when the receiver is a tracked
            // `const buf = Buffer.alloc(N)` local. Returns Ok(Some(reg)) on
            // success; falls through to the runtime dispatch for all other
            // Buffer methods or untracked receivers.
            if is_buffer_class {
                if let Some(lowered) = try_emit_buffer_read_intrinsic(ctx, object, property, args)?
                {
                    let materialized = crate::expr::materialize_js_value(
                        ctx,
                        lowered,
                        crate::native_value::MaterializationReason::FunctionAbi,
                    );
                    return Ok(Some(materialized));
                }
            }
            // #6386 fast path: `dv.getFloat64(off, le)` / `dv.setInt32(off, v)`
            // lowers to one `js_data_view_{get,set}_direct` call instead of
            // the generic dispatch tower. Fires for a statically-typed
            // DataView receiver AND for an unknown-typed receiver (a mutable
            // `var v = new DataView(b)` is widened to Any by the local-type
            // fixpoint) whose method name matches the accessor family — the
            // runtime entry re-validates the receiver against the DataView
            // registry and re-enters `js_native_call_method` otherwise, so a
            // non-DataView receiver that happens to share the method name
            // keeps its generic dispatch semantics (same #5525 guarded-
            // fast-path shape as typed-array index access).
            if matches!(class_name_opt.as_deref(), Some("DataView") | None) {
                if let Some(reg) = super::dataview_intrinsic::try_emit_data_view_accessor(
                    ctx,
                    object,
                    property,
                    args,
                    call_byte_offset,
                )? {
                    return Ok(Some(reg));
                }
            }
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Intern the method name and pass its heap string handle as the
            // static-name method id. The typed-feedback wrapper resolves the
            // id to bytes only at the runtime boundary.
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let key_handle_global = format!("@{}", entry.handle_global);
            let key_box = ctx.block().load(DOUBLE, &key_handle_global);
            let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
            let method_id = ctx
                .block()
                .and(I64, &key_bits, crate::nanbox::POINTER_MASK_I64);
            // Stack-allocate the args array if any. The alloca MUST live in
            // the function entry block — emitting it into the current block
            // (which may be a loop body) makes LLVM lower it as a runtime
            // `sub %rsp, N` that never gets restored, eating the stack at
            // ~16 bytes/iteration. See issue #167.
            let (args_ptr, args_len_str) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                let blk = ctx.block();
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, v, &slot);
                }
                (buf_reg, n.to_string())
            };
            let site_id = emit_typed_feedback_register_site(
                ctx,
                TypedFeedbackKind::MethodCall,
                property,
                TypedFeedbackContract::method_call(),
            );
            // #5247: record the source location right before the dynamic
            // dispatch so a thrown "X is not a function" TypeError carries
            // `at <file>:<line>`. Args are already lowered above, so a nested
            // call's location no longer shadows this one.
            crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
            let blk = ctx.block();
            return Ok(Some(blk.call(
                DOUBLE,
                "js_typed_feedback_native_call_method_by_id",
                &[
                    (I64, &site_id),
                    (DOUBLE, &recv_box),
                    (I64, &method_id),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            )));
        }
    }
    Ok(None)
}

fn is_message_port_closure_method(object: &Expr, property: &str) -> bool {
    if !matches!(
        property,
        "postMessage"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "addEventListener"
            | "removeEventListener"
            | "close"
            | "start"
            | "ref"
            | "unref"
            | "hasRef"
    ) {
        return false;
    }

    matches!(
        object,
        Expr::PropertyGet { property: port, .. } if matches!(port.as_str(), "port1" | "port2")
    )
}

/// `C.prop(args)` where `prop` names a static ACCESSOR (`static get prop()`)
/// anywhere on `cls_name`'s chain is NOT a static-method dispatch: it must READ
/// the accessor (running the getter, which yields a function) and CALL that
/// function with `args`. We emit that value-call via the closure-callee
/// fallthrough — which lowers the callee `PropertyGet{<class>, prop}` (invoking
/// the getter with `this` bound to the class) and dispatches the result through
/// `js_closure_call` — instead of letting a downstream by-name dispatcher
/// (`js_native_call_method` / `js_class_static_method_call`) silently drop the
/// call (those miss because the name lives in CLASS_STATIC_ACCESSORS, not
/// CLASS_STATIC_METHODS). Returns `None` when `prop` is not a static accessor on
/// the chain. Refs test262 language/arguments-object cls-*-static-* getter calls.
//
/// Returns WHICH builtin collection the class ultimately extends — i.e. a
/// source-compiled `class X extends Map {}` (instances get a hidden Map/Set
/// backing at `super()` via `js_map_set_subclass_init` and their collection
/// methods dispatch through the runtime, not the class vtable). The result is
/// (`"Map"`/`"Set"`/`"WeakMap"`/`"WeakSet"`), or `None`. Method routing is
/// kind-specific: `class M extends Map`
/// calling `.add()`/`.union()` (Set methods) must NOT be forced through the Map
/// backing (which returns `undefined`) — it should fall through to the normal
/// missing-method / user-method path.
pub fn class_builtin_collection_kind(ctx: &FnCtx<'_>, cls_name: &str) -> Option<&'static str> {
    fn normalize(name: &str) -> Option<&'static str> {
        match name {
            "Map" => Some("Map"),
            "Set" => Some("Set"),
            "WeakMap" => Some("WeakMap"),
            "WeakSet" => Some("WeakSet"),
            _ => None,
        }
    }
    let mut cur = Some(cls_name.to_string());
    let mut depth = 0usize;
    while let Some(c) = cur {
        if depth > 32 {
            break;
        }
        let Some(ci) = ctx.classes.get(&c) else {
            // The chain reached a name codegen doesn't track — it may be the
            // builtin `Map`/`Set` heritage itself.
            return normalize(c.as_str());
        };
        if let Some(parent) = ci.extends_name.as_deref() {
            if let Some(kind) = normalize(parent) {
                return Some(kind);
            }
        }
        cur = ci.extends_name.clone();
        depth += 1;
    }
    None
}

/// True when `method` is a backing-store collection method for `kind` (the
/// builtin a class extends). Map/WeakMap-only vs Set/WeakSet-only methods are
/// kept distinct so `class M extends Map` calling a Set method (`.add`,
/// `.union`, …) is NOT mis-routed onto the Map backing.
fn is_collection_method_for_kind(kind: &str, method: &str) -> bool {
    let shared = matches!(method, "has" | "delete" | "clear" | "forEach");
    match kind {
        "Map" => shared || matches!(method, "get" | "set" | "keys" | "values" | "entries"),
        "WeakMap" => matches!(method, "has" | "get" | "set" | "delete"),
        "Set" => {
            shared
                || matches!(
                    method,
                    "add"
                        | "keys"
                        | "values"
                        | "entries"
                        | "union"
                        | "intersection"
                        | "difference"
                        | "symmetricDifference"
                        | "isSubsetOf"
                        | "isSupersetOf"
                        | "isDisjointFrom"
                )
        }
        "WeakSet" => matches!(method, "has" | "add" | "delete"),
        _ => false,
    }
}

/// True when `cls_name` is a user class that (transitively) extends the builtin
/// `Array` — `class Stack extends Array`, `class B extends Stack`, … The chain
/// walk mirrors [`class_builtin_collection_kind`]: any ancestor whose
/// `extends_name` is `Array` (or `ReadonlyArray`) marks the class as an Array
/// subclass. `Array` itself is not a user class (never in `ctx.classes`), so the
/// heritage is detected by name at the end of the chain.
pub fn class_extends_builtin_array(ctx: &FnCtx<'_>, cls_name: &str) -> bool {
    fn is_array_name(name: &str) -> bool {
        matches!(name, "Array" | "ReadonlyArray")
    }
    // A receiver whose static type IS `Array` is a real array, not a subclass —
    // it has its own array-method lowering and must not be treated here.
    if is_array_name(cls_name) {
        return false;
    }
    let mut cur = Some(cls_name.to_string());
    let mut depth = 0usize;
    while let Some(c) = cur {
        if depth > 32 {
            break;
        }
        let Some(ci) = ctx.classes.get(&c) else {
            // Reached a name codegen doesn't track — it may be the builtin
            // `Array` heritage itself.
            return is_array_name(c.as_str());
        };
        if let Some(parent) = ci.extends_name.as_deref() {
            if is_array_name(parent) {
                return true;
            }
        }
        cur = ci.extends_name.clone();
        depth += 1;
    }
    false
}

/// The `URLSearchParams.prototype` methods handled by the runtime's
/// `dispatch_url_search_params` over a subclass instance's hidden native
/// backing. A name outside this set stays on the normal class-dispatch path (a
/// genuine user override already resolved earlier). `size` is a getter, not a
/// method, so it is intentionally absent.
fn is_url_search_params_method(method: &str) -> bool {
    matches!(
        method,
        "get"
            | "getAll"
            | "has"
            | "set"
            | "append"
            | "delete"
            | "toString"
            | "entries"
            | "keys"
            | "values"
            | "forEach"
            | "sort"
    )
}

/// The inherited `Array.prototype` methods the runtime's spec-generic array-like
/// engine handles for a `class X extends Array` instance (union of the mutator
/// and read dispatch families in `perry-runtime`'s `array::generic`). A method
/// name outside this set is left on the normal class-dispatch path (a genuine
/// user method already resolved earlier; anything else keeps its prior behavior).
fn is_array_like_method(method: &str) -> bool {
    matches!(
        method,
        // mutators (try_object_arraylike_mutator / run_object_mutator)
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "reverse"
            | "splice"
            | "sort"
            | "concat"
            // reads (dispatch_arraylike_read_method)
            | "forEach"
            | "map"
            | "filter"
            | "some"
            | "every"
            | "find"
            | "findIndex"
            | "findLast"
            | "findLastIndex"
            | "reduce"
            | "reduceRight"
            | "indexOf"
            | "lastIndexOf"
            | "includes"
            | "at"
            | "join"
            | "slice"
    )
}

pub fn try_lower_class_static_accessor_call(
    ctx: &mut FnCtx<'_>,
    cls_name: &str,
    property: &str,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let mut cur = Some(cls_name.to_string());
    while let Some(c) = cur {
        let Some(ci) = ctx.classes.get(&c) else {
            break;
        };
        if ci.static_accessor_names.iter().any(|n| n == property) {
            return try_lower_closure_call_fallthrough(ctx, callee, args);
        }
        cur = ci.extends_name.clone();
    }
    Ok(None)
}

/// A method-call receiver that must be lowered exactly once because
/// re-evaluating it would re-run side effects (or re-construct a fresh
/// value). The closure-call fallthrough otherwise lowers the receiver
/// twice — once for the `this` binding and once inside the full callee
/// PropertyGet — which double-runs these.
fn receiver_must_eval_once(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Call { .. }
            | Expr::CallSpread { .. }
            | Expr::NativeMethodCall { .. }
            | Expr::StaticMethodCall { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::Await(_)
    )
}

pub fn try_lower_closure_call_fallthrough(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Fallthrough: assume the callee evaluates to a closure value at
    // runtime and dispatch through `js_closure_call<N>`. This catches:
    //   - LocalGet of an `: any`-typed local that the static check missed
    //   - Nested calls like `curry(1)(2)(3)` where the callee is itself
    //     a Call returning a function
    //   - PropertyGet on a class instance whose property is a closure
    //
    // The runtime checks the closure header on its own — if the value
    // isn't actually a closure, js_closure_call<N> handles the error.
    // Issue #519: when the callee shape is `recv.method(args)` (a
    // PropertyGet) — i.e. a method-style invocation — bind the
    // receiver as the implicit `this` for the duration of the call.
    // Non-arrow function bodies (including FuncRef wrappers) read
    // `this` via `js_implicit_this_get` when their lexical
    // this_stack is empty (codegen Expr::This fallback). Without
    // this save/set/restore, hono's `RegExpRouter.match = match`
    // (where `match` is an imported function declaration whose
    // body does `this.buildAllMatchers()`) sees `this = undefined`
    // and TypeErrors out at the first chained method call.
    //
    // We evaluate `object` once into a fresh slot so that
    // (a) it's only side-effect-evaluated once, and
    // (b) the lowered `callee` (which re-reads `object` to get the
    //     property) and the IMPLICIT_THIS save/set both see the
    //     same receiver value.
    //
    // The `this`-binding / closure-unbox setup below is arity-independent;
    // only the final dispatch differs. Arities 0..=16 use the per-arity
    // `js_closure_call{N}` register helpers (fast path); arities > 16 (no
    // `js_closure_call17`+ exists) marshal the args through a stack buffer
    // and dispatch via `js_closure_call_array`. Refs #3527 (qs's recursive
    // `stringify` self-calls with 18 args).
    // When the receiver is itself side-effecting (a call / construct /
    // await — e.g. a chained `C.staticMethod(g()).next()`), it MUST be
    // evaluated exactly once. The straightforward shape below lowers
    // `object` for the `this` binding AND lowers the full `callee`
    // PropertyGet for the closure value, which re-lowers `object` and so
    // re-runs its side effects. That double-stepped `g()` in test262
    // `language/{statements,expressions}/class/dstr/async-gen-meth-static-*`
    // (`C.method(g()).next()`). For a side-effecting receiver, evaluate it
    // once here and read the method off that value directly.
    //
    // #5247: capture this call's source byte offset NOW, before the receiver
    // and arguments are lowered (a nested call in either would overwrite the
    // shared pending offset). The `js_closure_unbox_callee_checked` dispatch
    // below throws `TypeError: value is not a function` for a non-callable
    // callee (`const f: any = 5; f()`, nanoid/yup's failure shape), and that
    // throw renders `CURRENT_CALL_LOCATION` via `make_stack`. Replaying the
    // offset as `js_set_call_location` right before the dispatch gives the
    // throw an `at <file>:<line>` frame under `--debug-symbols`. `0` (and the
    // default build) → no emission, unchanged `<anonymous>` frame.
    let call_byte_offset = ctx.strings.pending_call_offset();
    let prelowered_recv: Option<(String, String)> =
        if let Expr::PropertyGet {
            object, property, ..
        } = callee
        {
            if receiver_must_eval_once(object.as_ref()) {
                Some((lower_expr(ctx, object)?, property.clone()))
            } else {
                None
            }
        } else {
            None
        };

    let method_recv: Option<String> = if let Some((ref obj_v, _)) = prelowered_recv {
        Some(obj_v.clone())
    } else if let Expr::PropertyGet { object, .. } = callee {
        // Skip the method-binding when the receiver is a global,
        // namespace import, or NativeModuleRef — those aren't
        // user objects and shouldn't influence `this`.
        if matches!(
            object.as_ref(),
            Expr::GlobalGet(_) | Expr::NativeModuleRef(_) | Expr::ExternFuncRef { .. }
        ) {
            None
        } else {
            Some(lower_expr(ctx, object)?)
        }
    } else {
        None
    };

    let recv_box = if let Some((ref obj_v, ref property)) = prelowered_recv {
        // Read `property` off the once-lowered receiver value via the
        // generic by-name getter (walks the prototype chain, so
        // `gen.next` etc. resolve). Mirrors the dynamic by-name read in
        // `expr::property_get::lower`.
        let key_idx = ctx.strings.intern(property);
        let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
        let blk = ctx.block();
        let obj_bits = blk.bitcast_double_to_i64(obj_v);
        let key_box = blk.load(DOUBLE, &key_handle_global);
        let key_bits = blk.bitcast_double_to_i64(&key_box);
        let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
        blk.call(
            DOUBLE,
            "js_object_get_field_by_name_f64",
            &[(I64, &obj_bits), (I64, &key_handle)],
        )
    } else {
        lower_expr(ctx, callee)?
    };
    let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
    for a in args {
        lowered_args.push(lower_expr(ctx, a)?);
    }
    let prev_this: Option<String> = if let Some(ref this_val) = method_recv {
        let blk = ctx.block();
        Some(blk.call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, this_val)]))
    } else if !matches!(callee, Expr::PropertyGet { .. }) {
        // Receiverless closure-value call (`fn()`, IIFE, `curry(1)(2)`):
        // OrdinaryCallBindThis binds `this` to undefined — without the
        // reset the enclosing method dispatch's IMPLICIT_THIS leaks into
        // the callee (#3576). Member-shaped callees keep their existing
        // receiver/skip behavior above.
        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let blk = ctx.block();
        Some(blk.call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &undef)]))
    } else {
        None
    };

    // #5247: record the source location right before the throw-capable
    // dispatch — after the receiver/args are lowered, so a nested-call
    // argument's location no longer shadows this one. Applies to both arity
    // branches below (the checked unbox throws in either). No-op default build.
    crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
    let result = if lowered_args.len() <= 16 {
        let blk = ctx.block();
        // #5504: tag-check the callee before masking to a closure pointer.
        // A non-callable value (number/string/bool/null/undefined) whose
        // low-48 bits form an in-range address would otherwise be handed to
        // `js_closure_callN` as a wild `*const ClosureHeader` and SIGSEGV on
        // the header read. The checked unbox throws `TypeError: value is not
        // a function` for any non-`POINTER_TAG` value.
        // #6475: a member-shaped call (`o.m(args)`) must rebind an
        // object-literal method's baked `this` capture slot to the receiver —
        // the slot wins over the IMPLICIT_THIS cell set above, so a method
        // inherited via `Object.setPrototypeOf(obj, proto)` otherwise runs
        // with `this` bound to the proto literal (effect's Pipeable
        // `TagClass.pipe(...)` composed against the wrong `this` and
        // HttpApiBuilder.group returned a curried function instead of a
        // Layer). The rebind variant is a no-op for closures that don't
        // capture `this`, so plain functions and arrows are untouched;
        // receiverless calls keep the plain checked unbox.
        let closure_handle = if let Some(ref this_val) = method_recv {
            blk.call(
                I64,
                "js_closure_unbox_callee_checked_rebind",
                &[(DOUBLE, &recv_box), (DOUBLE, this_val)],
            )
        } else {
            blk.call(
                I64,
                "js_closure_unbox_callee_checked",
                &[(DOUBLE, &recv_box)],
            )
        };
        let runtime_fn = format!("js_closure_call{}", lowered_args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        blk.call(DOUBLE, &runtime_fn, &call_args)
    } else {
        // #3527: > 16 args — stack-allocate a `[N x double]` array (entry-block
        // alloca, see #167), store each lowered arg, and dispatch through the
        // variadic `js_closure_call_array(closure_i64, args_ptr, argc)`. This
        // mirrors the `js_native_call_value` marshaling used elsewhere in
        // lower_call. `args_ptr` is non-null here since argc > 16 > 0.
        let n = lowered_args.len();
        let buf = ctx.func.alloca_entry_array(DOUBLE, n);
        let blk = ctx.block();
        for (i, v) in lowered_args.iter().enumerate() {
            let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
            blk.store(DOUBLE, v, &slot);
        }
        // #5504: tag-check the callee before masking to a closure pointer.
        // A non-callable value (number/string/bool/null/undefined) whose
        // low-48 bits form an in-range address would otherwise be handed to
        // `js_closure_callN` as a wild `*const ClosureHeader` and SIGSEGV on
        // the header read. The checked unbox throws `TypeError: value is not
        // a function` for any non-`POINTER_TAG` value.
        // #6475: a member-shaped call (`o.m(args)`) must rebind an
        // object-literal method's baked `this` capture slot to the receiver —
        // the slot wins over the IMPLICIT_THIS cell set above, so a method
        // inherited via `Object.setPrototypeOf(obj, proto)` otherwise runs
        // with `this` bound to the proto literal (effect's Pipeable
        // `TagClass.pipe(...)` composed against the wrong `this` and
        // HttpApiBuilder.group returned a curried function instead of a
        // Layer). The rebind variant is a no-op for closures that don't
        // capture `this`, so plain functions and arrows are untouched;
        // receiverless calls keep the plain checked unbox.
        let closure_handle = if let Some(ref this_val) = method_recv {
            blk.call(
                I64,
                "js_closure_unbox_callee_checked_rebind",
                &[(DOUBLE, &recv_box), (DOUBLE, this_val)],
            )
        } else {
            blk.call(
                I64,
                "js_closure_unbox_callee_checked",
                &[(DOUBLE, &recv_box)],
            )
        };
        let argc = n.to_string();
        blk.call(
            DOUBLE,
            "js_closure_call_array",
            &[(I64, &closure_handle), (PTR, &buf), (I64, &argc)],
        )
    };

    if let Some(prev) = prev_this {
        ctx.block()
            .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev)]);
    }
    Ok(Some(result))
}
