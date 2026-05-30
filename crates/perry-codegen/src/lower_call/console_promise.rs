//! Trailing `lower_call` branches:
//! - `console.log` / `console.info` / `console.warn` / …
//! - `Promise.resolve` / `.reject` / `.all` / `.race` / `.allSettled`
//!   plus `Array.fromAsync`
//! - Universal `js_native_call_method` PropertyGet dispatch
//! - Closure-call fallthrough (`recv()` where `recv` is a closure value)
//!
//! Plus the `util/types` predicate helper that bypasses the runtime
//! dispatcher for source-level `util.types.isPromise(x)` and friends after HIR
//! normalizes them to the canonical `util/types` module key.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{
    emit_typed_feedback_register_site, lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx,
    TypedFeedbackContract, TypedFeedbackKind,
};
use crate::nanbox::double_literal;
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
    let is_direct_util_types_module = module == "util/types" && class_name.is_none();
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
        "isPromise" => Some("js_util_types_is_promise"),
        "isArrayBuffer" => Some("js_util_types_is_array_buffer"),
        "isSharedArrayBuffer" => Some("js_util_types_is_shared_array_buffer"),
        "isAnyArrayBuffer" => Some("js_util_types_is_any_array_buffer"),
        "isArrayBufferView" => Some("js_util_types_is_array_buffer_view"),
        "isTypedArray" => Some("js_util_types_is_typed_array"),
        "isUint8Array" => Some("js_util_types_is_uint8_array"),
        "isInt8Array" => Some("js_util_types_is_int8_array"),
        "isInt16Array" => Some("js_util_types_is_int16_array"),
        "isUint16Array" => Some("js_util_types_is_uint16_array"),
        "isInt32Array" => Some("js_util_types_is_int32_array"),
        "isUint32Array" => Some("js_util_types_is_uint32_array"),
        "isFloat32Array" => Some("js_util_types_is_float32_array"),
        "isFloat64Array" => Some("js_util_types_is_float64_array"),
        "isUint8ClampedArray" => Some("js_util_types_is_uint8_clamped_array"),
        "isBigInt64Array" => Some("js_util_types_is_big_int64_array"),
        "isBigUint64Array" => Some("js_util_types_is_big_uint64_array"),
        "isMap" => Some("js_util_types_is_map"),
        "isMapIterator" => Some("js_util_types_is_map_iterator"),
        "isProxy" => Some("js_util_types_is_proxy"),
        "isSet" => Some("js_util_types_is_set"),
        "isSetIterator" => Some("js_util_types_is_set_iterator"),
        "isDate" => Some("js_util_types_is_date"),
        "isRegExp" => Some("js_util_types_is_reg_exp"),
        "isAsyncFunction" => Some("js_util_types_is_async_function"),
        "isGeneratorFunction" => Some("js_util_types_is_generator_function"),
        "isGeneratorObject" => Some("js_util_types_is_generator_object"),
        "isNativeError" => Some("js_util_types_is_native_error"),
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
    if let Expr::PropertyGet { object, property } = callee {
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
    if let Expr::PropertyGet { object, property } = callee {
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
        // `Array.fromAsync(input)` — Node 22+ static method.
        if is_global_constructor_expr(object, "Array") && property == "fromAsync" {
            if args.is_empty() {
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            let input = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            return Ok(Some(blk.call(
                DOUBLE,
                "js_array_from_async",
                &[(DOUBLE, &input)],
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
    if let Expr::PropertyGet { object, property } = callee {
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
        );
        let skip_native = matches!(object.as_ref(), Expr::GlobalGet(_))
            || matches!(object.as_ref(), Expr::NativeModuleRef(_))
            || (class_name_opt.is_some()
                && !is_buffer_class
                && !class_unknown_to_codegen
                && !is_well_known_proto_method);
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
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Intern the method name and reference its rodata byte global.
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len_str = entry.byte_len.to_string();
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
            let blk = ctx.block();
            return Ok(Some(blk.call(
                DOUBLE,
                "js_typed_feedback_native_call_method",
                &[
                    (I64, &site_id),
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            )));
        }
    }
    Ok(None)
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
    if args.len() <= 16 {
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
        let method_recv: Option<String> = if let Expr::PropertyGet { object, .. } = callee {
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

        let recv_box = lower_expr(ctx, callee)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        let prev_this: Option<String> = if let Some(ref this_val) = method_recv {
            let blk = ctx.block();
            Some(blk.call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, this_val)]))
        } else {
            None
        };
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &recv_box);
        let runtime_fn = format!("js_closure_call{}", args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        let result = blk.call(DOUBLE, &runtime_fn, &call_args);
        if let Some(prev) = prev_this {
            ctx.block()
                .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev)]);
        }
        return Ok(Some(result));
    }
    Ok(None)
}
