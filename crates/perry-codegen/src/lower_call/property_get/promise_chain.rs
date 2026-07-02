//! Promise `.then` / `.catch` / `.finally` PropertyGet dispatch.
//! Pure code move from `property_get.rs` — no behavior change.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64, FnCtx};
use crate::lower_array_method::lower_array_method;
use crate::lower_string_method::{is_known_string_method_name, lower_string_method};
use crate::nanbox::double_literal;
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_native_module_dynamic_index,
    is_promise_expr, is_set_expr, is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64};

/// Promise pointers are NaN-boxed with POINTER_TAG. We unbox to get the raw
/// i64 promise handle, then call the runtime `js_promise_then(promise,
/// on_fulfilled, on_rejected)` which returns a new promise handle that we
/// re-box with POINTER_TAG. `.catch(cb)` is sugar for `.then(undefined, cb)`.
pub(crate) fn try_lower_promise_chain_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    if matches!(property, "then" | "catch" | "finally") && is_promise_expr(ctx, object) {
        match property {
            "then"
                if !args.is_empty() => {
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
                    } = object
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
            "catch"
                if !args.is_empty() => {
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
            "finally"
                // .finally(cb) — per spec: call cb() ignoring its return value,
                // then propagate the upstream value/reason unchanged.
                // Routes through js_promise_finally which wraps cb in
                // fulfill/reject proxy closures that call cb() and then
                // return the upstream value (or re-throw the upstream reason).
                if !args.is_empty() => {
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
            _ => {}
        }
    }
    Ok(None)
}
