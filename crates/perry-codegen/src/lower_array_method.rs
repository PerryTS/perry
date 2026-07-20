//! Array method lowering for array-typed receivers.
//!
//! Contains `lower_array_method` which dispatches `.pop()`, `.join()`,
//! `.some()`, `.every()`, `.toString()`, `.concat()`, `.sort()`,
//! `.reverse()`, `.flat()`, `.flatMap()`, plus safety-net handlers for
//! methods that normally arrive as HIR variants but may reach here as
//! generic MethodCall when the HIR lowering doesn't recognize the pattern.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{
    emit_root_nanbox_store_on_block, emit_write_barrier, lower_expr, nanbox_pointer_inline,
    nanbox_string_inline, unbox_str_handle, unbox_to_i64, FnCtx,
};
use crate::nanbox::{double_literal, TAG_UNDEFINED};
use crate::types::{DOUBLE, I32, I64, PTR};

/// Write the (possibly reallocated) array head produced by a growing mutator
/// (`unshift`, …) back to whichever storage backs a single-local receiver:
/// boxed cell, closure capture, plain local slot, or module global.
///
/// This is the same routing `Expr::ArrayPush` performs (see
/// `expr/array_push.rs`). Growth installs a forwarding stub at the old header,
/// but a boxed async/closure local is read through its box cell and never
/// observes that forwarding, so without this write-back a growing `unshift`
/// inside an async fn left the variable reading `undefined` (#6229).
pub(crate) fn emit_grow_mutator_writeback(
    ctx: &mut FnCtx<'_>,
    array_id: u32,
    new_box: &str,
) -> Result<()> {
    if ctx.boxed_vars.contains(&array_id) {
        // Boxed var: the slot / capture holds the BOX pointer; update the box
        // content so every closure sharing the box sees the new head.
        if let Some(&capture_idx) = ctx.closure_captures.get(&array_id) {
            let closure_ptr = ctx.current_closure_ptr.clone().ok_or_else(|| {
                anyhow::anyhow!("unshift boxed capture but no current_closure_ptr")
            })?;
            let idx_str = capture_idx.to_string();
            let blk = ctx.block();
            let box_ptr = blk.call(
                I64,
                "js_closure_get_capture_bits",
                &[(I64, &closure_ptr), (I32, &idx_str)],
            );
            let new_bits = blk.bitcast_double_to_i64(new_box);
            blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
            emit_write_barrier(ctx, &box_ptr, &new_bits);
            return Ok(());
        } else if let Some(slot) = ctx.locals.get(&array_id).cloned() {
            let blk = ctx.block();
            let box_ptr = blk.load(I64, &slot);
            let new_bits = blk.bitcast_double_to_i64(new_box);
            blk.call_void("js_box_set_bits", &[(I64, &box_ptr), (I64, &new_bits)]);
            emit_write_barrier(ctx, &box_ptr, &new_bits);
            return Ok(());
        }
        // Boxed but no box location in THIS context (module global reached
        // directly from a nested fn) — fall through to the global store.
    }
    if let Some(&capture_idx) = ctx.closure_captures.get(&array_id) {
        let closure_ptr = ctx
            .current_closure_ptr
            .clone()
            .ok_or_else(|| anyhow::anyhow!("unshift capture but no current_closure_ptr"))?;
        let idx_str = capture_idx.to_string();
        let new_bits = ctx.block().bitcast_double_to_i64(new_box);
        ctx.block().call_void(
            "js_closure_set_capture_bits",
            &[(I64, &closure_ptr), (I32, &idx_str), (I64, &new_bits)],
        );
        emit_write_barrier(ctx, &closure_ptr, &new_bits);
    } else if let Some(slot) = ctx.locals.get(&array_id).cloned() {
        ctx.block().store(DOUBLE, new_box, &slot);
    } else if let Some(global_name) = ctx.module_globals.get(&array_id).cloned() {
        let g_ref = format!("@{}", global_name);
        emit_root_nanbox_store_on_block(ctx.block(), new_box, &g_ref);
    }
    // Non-local receiver (property chain): rely on in-place mutation /
    // forwarding, matching the prior behavior.
    Ok(())
}

/// Lower `arr.method(args…)` for an array-typed receiver. Currently
/// supported: `pop`, `join`. `push` is handled separately by the HIR
/// `Expr::ArrayPush` variant (Phase B.7).
pub(crate) fn lower_array_method(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<String> {
    let recv_box = lower_expr(ctx, object)?;

    match property {
        "pop" => {
            // No-arg method; JS ignores extras but evaluates them for side
            // effects before the call. Evaluate then discard.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Returns f64 directly (the popped element, NaN if empty).
            Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &recv_handle)]))
        }
        "join" => {
            let sep_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result_handle = blk.call(
                I64,
                "js_array_join_value",
                &[(I64, &recv_handle), (DOUBLE, &sep_box)],
            );
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "some" | "every" => {
            // An explicit `thisArg` (2nd argument) must bind the callback's
            // `this`; the dense helpers don't take one (they bind undefined),
            // so route through the generic array-like engine, which does
            // (real-array receivers keep a fast element path there).
            if args.len() >= 2 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let this_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let gen_fn = if property == "some" {
                    "js_arraylike_some"
                } else {
                    "js_arraylike_every"
                };
                return Ok(blk.call(
                    DOUBLE,
                    gen_fn,
                    &[(DOUBLE, &recv_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
                ));
            }

            // `arr.some()` / `arr.every()` with no callback must throw a
            // runtime TypeError ("undefined is not a function"), not fail to
            // compile — pad a missing callback with `undefined` and let
            // `js_validate_array_callback` raise it. (A trailing `thisArg`
            // is accepted but not yet applied.)
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let runtime_fn = if property == "some" {
                "js_array_some"
            } else {
                "js_array_every"
            };
            Ok(blk.call(
                DOUBLE,
                runtime_fn,
                &[(I64, &recv_handle), (I64, &cb_handle)],
            ))
        }
        "toString" => {
            // arr.toString() == arr.join(",")
            let key_idx = ctx.strings.intern(",");
            let handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
            let blk = ctx.block();
            let sep_box = blk.load(DOUBLE, &handle_global);
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Interned literal "," — heap allocated at module init, so
            // `unbox_to_i64` would technically work, but routing through
            // `unbox_str_handle` keeps the path uniform with the `join`
            // arm and is robust if interning ever changes to SSO-eligible.
            let sep_handle = unbox_str_handle(blk, &sep_box);
            let result_handle = blk.call(
                I64,
                "js_array_join",
                &[(I64, &recv_handle), (I64, &sep_handle)],
            );
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "concat" => {
            // #2805: arr.concat(...args) — spec-complete, non-mutating, variadic.
            // Issue #637: pre-fix this called `js_array_concat` (mutating
            // — used internally by spread-into-array desugar) which wrote
            // `other`'s elements into `recv`'s storage. When `recv` was the
            // result of `Object.keys(privateField)` (which fast-paths to
            // returning `(*obj).keys_array` directly — see
            // `js_object_keys`), the user-visible `k.concat(k2)` call
            // mutated the source object's keys_array, corrupting Object.keys
            // output for that object thereafter and aliasing it with newly-
            // allocated keys_arrays of OTHER objects via GC reuse. The
            // user-visible `.concat()` is spec-non-mutating; route to the
            // dedicated non-mutating helper.
            //
            // Lower every argument into an alloca buffer of raw NaN-boxed
            // doubles, then call `js_array_concat_variadic(recv, ptr, count)`
            // which applies Symbol.isConcatSpreadable / array-like spreading
            // and always returns a fresh array (receiver unchanged). Mirrors
            // the `unshift` variadic pattern below.
            let mut item_vals: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                item_vals.push(lower_expr(ctx, a)?);
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let n = item_vals.len();
            let (buf_reg, count_str) = if n == 0 {
                // No args: pass a null buffer + 0 count (concat() returns a copy).
                ("null".to_string(), "0".to_string())
            } else {
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, format!("{}", n))
            };
            let result = blk.call(
                I64,
                "js_array_concat_variadic",
                &[(I64, &recv_handle), (PTR, &buf_reg), (I32, &count_str)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "sort" => {
            // arr.sort() — default comparator (stringwise compare).
            // arr.sort(cb) — custom comparator path.
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = if args.is_empty() {
                blk.call(I64, "js_array_sort_default", &[(I64, &recv_handle)])
            } else {
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                // #2796: validate comparator (function | undefined) before sorting.
                let cb_handle = blk.call(I64, "js_validate_array_comparator", &[(DOUBLE, &cb_box)]);
                blk.call(
                    I64,
                    "js_array_sort_with_comparator",
                    &[(I64, &recv_handle), (I64, &cb_handle)],
                )
            };
            Ok(nanbox_pointer_inline(ctx.block(), &result))
        }
        "reverse" => {
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_reverse", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "copyWithin" => {
            // ECMA-262 §23.1.3.5 `arr.copyWithin(target, start?, end?)`.
            // The HIR `Expr::ArrayCopyWithin` lowering in `expr_call.rs:3461`
            // only fires when the receiver is a local — literal-receiver
            // calls (`[1,2,3,4,5].copyWithin(0, 1)`) fall through to here.
            // Without this arm the call returned the receiver unchanged,
            // because the general method-dispatch fallback doesn't know
            // about copyWithin and silently no-op'd.
            if args.is_empty() {
                bail!("perry-codegen: Array.copyWithin expects 1-3 args, got 0",);
            }
            let target_d = lower_expr(ctx, &args[0])?;
            let start_d = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(0.0)
            };
            let (has_end_str, end_d) = if args.len() >= 3 {
                let v = lower_expr(ctx, &args[2])?;
                ("1".to_string(), v)
            } else {
                ("0".to_string(), "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_array_copy_within",
                &[
                    (I64, &recv_handle),
                    (DOUBLE, &target_d),
                    (DOUBLE, &start_d),
                    (I32, &has_end_str),
                    (DOUBLE, &end_d),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "flat" => {
            // ECMA-262 §23.1.3.10 `arr.flat(depth?)`. Default depth = 1.
            // The depth-aware path routes to `js_array_flat_depth` (handles
            // 0 = shallow copy, Infinity = full recursion); 0-arg keeps
            // the legacy `js_array_flat` fast path.
            if args.is_empty() {
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let result = blk.call(I64, "js_array_flat", &[(I64, &recv_handle)]);
                Ok(nanbox_pointer_inline(blk, &result))
            } else {
                let depth_d = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let result = blk.call(
                    I64,
                    "js_array_flat_depth",
                    &[(I64, &recv_handle), (DOUBLE, &depth_d)],
                );
                Ok(nanbox_pointer_inline(blk, &result))
            }
        }
        "flatMap" => {
            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let result = blk.call(
                I64,
                "js_array_flatMap",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // -------- Safety-net handlers for methods that normally arrive --------
        // as HIR variants but may reach here as generic MethodCall when
        // the HIR lowering doesn't recognize the pattern.
        "find" => {
            // An explicit `thisArg` (2nd argument) must bind the callback's
            // `this`; the dense helpers don't take one (they bind undefined),
            // so route through the generic array-like engine, which does
            // (real-array receivers keep a fast element path there).
            if args.len() >= 2 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let this_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let gen_fn = "js_arraylike_find";
                return Ok(blk.call(
                    DOUBLE,
                    gen_fn,
                    &[(DOUBLE, &recv_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
                ));
            }

            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_array_find",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            ))
        }
        "findIndex" => {
            // An explicit `thisArg` (2nd argument) must bind the callback's
            // `this`; the dense helpers don't take one (they bind undefined),
            // so route through the generic array-like engine, which does
            // (real-array receivers keep a fast element path there).
            if args.len() >= 2 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let this_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let gen_fn = "js_arraylike_findIndex";
                return Ok(blk.call(
                    DOUBLE,
                    gen_fn,
                    &[(DOUBLE, &recv_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
                ));
            }

            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let i32_v = blk.call(
                I32,
                "js_array_findIndex",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "findLast" => {
            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            Ok(blk.call(
                DOUBLE,
                "js_array_find_last",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            ))
        }
        "findLastIndex" => {
            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let i32_v = blk.call(
                I32,
                "js_array_find_last_index",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "reduce" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: Array.reduce expects 1-2 args, got {}",
                    args.len()
                );
            }
            // 0-arg → runtime TypeError (callback validation on undefined),
            // not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let (has_initial, initial_box) = if args.len() == 2 {
                let init = lower_expr(ctx, &args[1])?;
                (1i32, init)
            } else {
                (0i32, "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let has_init_str = format!("{}", has_initial);
            Ok(blk.call(
                DOUBLE,
                "js_array_reduce",
                &[
                    (I64, &recv_handle),
                    (I64, &cb_handle),
                    (I32, &has_init_str),
                    (DOUBLE, &initial_box),
                ],
            ))
        }
        "reduceRight" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: Array.reduceRight expects 1-2 args, got {}",
                    args.len()
                );
            }
            // 0-arg → runtime TypeError (callback validation on undefined),
            // not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let (has_initial, initial_box) = if args.len() == 2 {
                let init = lower_expr(ctx, &args[1])?;
                (1i32, init)
            } else {
                (0i32, "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let has_init_str = format!("{}", has_initial);
            Ok(blk.call(
                DOUBLE,
                "js_array_reduce_right",
                &[
                    (I64, &recv_handle),
                    (I64, &cb_handle),
                    (I32, &has_init_str),
                    (DOUBLE, &initial_box),
                ],
            ))
        }
        "map" => {
            // An explicit `thisArg` (2nd argument) must bind the callback's
            // `this`; the dense helpers don't take one (they bind undefined),
            // so route through the generic array-like engine, which does
            // (real-array receivers keep a fast element path there).
            if args.len() >= 2 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let this_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let gen_fn = "js_arraylike_map";
                return Ok(blk.call(
                    DOUBLE,
                    gen_fn,
                    &[(DOUBLE, &recv_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
                ));
            }

            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            // `map` uses a receiver-aware validator (TypedArray.map renders its
            // non-callable message differently than Array.prototype.map).
            let cb_handle = blk.call(
                I64,
                "js_validate_array_map_callback",
                &[(I64, &recv_handle), (DOUBLE, &cb_box)],
            );
            let result = blk.call(
                I64,
                "js_array_map",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "filter" => {
            // An explicit `thisArg` (2nd argument) must bind the callback's
            // `this`; the dense helpers don't take one (they bind undefined),
            // so route through the generic array-like engine, which does
            // (real-array receivers keep a fast element path there).
            if args.len() >= 2 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let this_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let gen_fn = "js_arraylike_filter";
                return Ok(blk.call(
                    DOUBLE,
                    gen_fn,
                    &[(DOUBLE, &recv_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
                ));
            }

            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            let result = blk.call(
                I64,
                "js_array_filter",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "forEach" => {
            // An explicit `thisArg` (2nd argument) must bind the callback's
            // `this`; the dense helpers don't take one (they bind undefined),
            // so route through the generic array-like engine, which does
            // (real-array receivers keep a fast element path there).
            if args.len() >= 2 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let this_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let gen_fn = "js_arraylike_forEach";
                return Ok(blk.call(
                    DOUBLE,
                    gen_fn,
                    &[(DOUBLE, &recv_box), (DOUBLE, &cb_box), (DOUBLE, &this_box)],
                ));
            }

            // 0-arg → runtime TypeError (pad undefined), not compile-fail.
            let cb_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // #4091: throw TypeError for a non-callable callback before iterating.
            let cb_handle = blk.call(I64, "js_validate_array_callback", &[(DOUBLE, &cb_box)]);
            blk.call_void(
                "js_array_forEach",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            // forEach returns undefined
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "includes" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: Array.includes expects 1-2 args, got {}",
                    args.len()
                );
            }
            // 0-arg → includes(undefined), not compile-fail.
            let val_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            // #2804: optional fromIndex (2nd arg). has_from=1 + lowered index
            // when present; otherwise has_from=0 with a placeholder DOUBLE.
            let (from_box, has_from) = if args.len() == 2 {
                (lower_expr(ctx, &args[1])?, "1")
            } else {
                (val_box.clone(), "0")
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Use `js_array_includes_jsvalue` for deep equality so
            // string values stored in arrays (from e.g. `Object.keys()`
            // or `Object.getOwnPropertyNames()`) match by content, not
            // pointer identity. The `*_f64` variant compares raw bits
            // which fails for strings allocated at different sites.
            let i32_v = blk.call(
                I32,
                "js_array_includes_jsvalue",
                &[
                    (I64, &recv_handle),
                    (DOUBLE, &val_box),
                    (DOUBLE, &from_box),
                    (I32, has_from),
                ],
            );
            // Convert i32 boolean to NaN-boxed true/false
            let bit = blk.icmp_ne(I32, &i32_v, "0");
            let tagged = blk.select(
                "i1",
                &bit,
                I64,
                crate::nanbox::TAG_TRUE_I64,
                crate::nanbox::TAG_FALSE_I64,
            );
            Ok(blk.bitcast_i64_to_double(&tagged))
        }
        "indexOf" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: Array.indexOf expects 1-2 args, got {}",
                    args.len()
                );
            }
            // 0-arg → search for `undefined` from index 0, not compile-fail.
            let val_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            // #2804: optional fromIndex (2nd arg). has_from=1 + lowered index
            // when present; otherwise has_from=0 with a placeholder DOUBLE.
            let (from_box, has_from) = if args.len() == 2 {
                (lower_expr(ctx, &args[1])?, "1")
            } else {
                (val_box.clone(), "0")
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Issue #214: route through `_jsvalue` so string elements
            // match by content (handles SSO + heap-string mixed arrays
            // — `arr.indexOf("hello")` on a `JSON.parse(...)`-derived
            // string array returned -1 because the SSO element bits
            // never bit-equal the heap-string needle bits). Mirrors
            // the existing `includes` arm.
            let i64_v = blk.call(
                I64,
                "js_array_indexOf_jsvalue",
                &[
                    (I64, &recv_handle),
                    (DOUBLE, &val_box),
                    (DOUBLE, &from_box),
                    (I32, has_from),
                ],
            );
            Ok(blk.sitofp(I64, &i64_v, DOUBLE))
        }
        "lastIndexOf" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: Array.lastIndexOf expects 1-2 args, got {}",
                    args.len()
                );
            }
            // 0-arg → search for `undefined`, not compile-fail.
            let val_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            // Optional fromIndex: with has_from=1 pass the lowered index;
            // when absent pass has_from=0 (runtime defaults to length-1) and
            // reuse `val_box` as an ignored placeholder DOUBLE operand.
            let (from_box, has_from) = if args.len() == 2 {
                (lower_expr(ctx, &args[1])?, "1")
            } else {
                (val_box.clone(), "0")
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let i64_v = blk.call(
                I64,
                "js_array_last_index_of_jsvalue",
                &[
                    (I64, &recv_handle),
                    (DOUBLE, &val_box),
                    (DOUBLE, &from_box),
                    (I32, has_from),
                ],
            );
            Ok(blk.sitofp(I64, &i64_v, DOUBLE))
        }
        "at" => {
            // `arr.at()` with no index → `at(undefined)` → index 0, not a
            // compile error.
            let idx_box = if let Some(arg) = args.first() {
                lower_expr(ctx, arg)?
            } else {
                crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            Ok(blk.call(
                DOUBLE,
                "js_array_at",
                &[(I64, &recv_handle), (DOUBLE, &idx_box)],
            ))
        }
        "slice" => {
            if args.len() > 2 {
                bail!(
                    "perry-codegen: Array.slice expects 0-2 args, got {}",
                    args.len()
                );
            }
            let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let start_value = if args.is_empty() {
                "0.0".to_string()
            } else {
                lower_expr(ctx, &args[0])?
            };
            let end_value = if args.len() == 2 {
                lower_expr(ctx, &args[1])?
            } else {
                undefined
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_array_slice_values",
                &[
                    (I64, &recv_handle),
                    (DOUBLE, &start_value),
                    (DOUBLE, &end_value),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "shift" => {
            // No-arg method; extras are evaluated for side effects then ignored.
            for extra in args.iter() {
                let _ = lower_expr(ctx, extra)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            Ok(blk.call(DOUBLE, "js_array_shift_f64", &[(I64, &recv_handle)]))
        }
        "fill" => {
            // ECMA-262 Array.prototype.fill(value, start?, end?). The
            // 2-/3-arg forms route through `js_array_fill_range` which
            // applies the spec's negative-index + clamp rules and fills
            // `[start, end)`. The 1-arg form keeps the existing
            // whole-array fast path.
            match args.len() {
                // `arr.fill()` with no value fills the whole array with
                // `undefined` (ECMA-262 step: value defaults to undefined).
                0 | 1 => {
                    let val_box = if let Some(arg) = args.first() {
                        lower_expr(ctx, arg)?
                    } else {
                        crate::nanbox::double_literal(f64::from_bits(TAG_UNDEFINED))
                    };
                    let blk = ctx.block();
                    let recv_handle = unbox_to_i64(blk, &recv_box);
                    let result = blk.call(
                        I64,
                        "js_array_fill",
                        &[(I64, &recv_handle), (DOUBLE, &val_box)],
                    );
                    Ok(nanbox_pointer_inline(blk, &result))
                }
                2 | 3 => {
                    let val_box = lower_expr(ctx, &args[0])?;
                    let start_d = lower_expr(ctx, &args[1])?;
                    let end_d = if args.len() == 3 {
                        lower_expr(ctx, &args[2])?
                    } else {
                        // Default end = +Infinity → clamps to length in the runtime.
                        crate::nanbox::double_literal(f64::INFINITY)
                    };
                    let blk = ctx.block();
                    let recv_handle = unbox_to_i64(blk, &recv_box);
                    let result = blk.call(
                        I64,
                        "js_array_fill_range",
                        &[
                            (I64, &recv_handle),
                            (DOUBLE, &val_box),
                            (DOUBLE, &start_d),
                            (DOUBLE, &end_d),
                        ],
                    );
                    Ok(nanbox_pointer_inline(blk, &result))
                }
                _ => bail!(
                    "perry-codegen: Array.fill expects 1-3 args, got {}",
                    args.len()
                ),
            }
        }
        "unshift" => {
            // #2814 + Issue #656: returns the new array length per ECMA-262.
            // 0 args -> no mutation per spec, but frozen/non-writable guards
            // must still fire (ECMA-262 §23.1.3.31 step 8 always calls Set
            // [[length]] even for 0-arg). Route through the variadic helper
            // with null/0 so the runtime guards run. N args -> insert all
            // items at the front in source order via the variadic helper.
            // The (possibly reallocated) array forwards from its old pointer,
            // so in-place mutation stays visible to the receiver slot.
            // Materialize the args (if any) into an alloca buffer, then call
            // the variadic helper (preserves source order).
            let (buf_ptr, count_str) = if args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let mut item_vals: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    item_vals.push(lower_expr(ctx, a)?);
                }
                let n = item_vals.len();
                let blk = ctx.block();
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, format!("{}", n))
            };
            let recv_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &recv_box)
            };
            let new_handle = ctx.block().call(
                I64,
                "js_array_unshift_variadic",
                &[(I64, &recv_handle), (PTR, &buf_ptr), (I32, &count_str)],
            );
            // #6229: write the (possibly reallocated) header back to the
            // receiver's storage. Growth installs a forwarding stub on the old
            // header, but a boxed async/closure local is read through its box
            // cell and never observes that forwarding, so a growing `unshift`
            // inside an async fn left the variable reading `undefined`. Route
            // through the same storage resolution `ArrayPush` uses.
            if let Expr::LocalGet(array_id) = object {
                let new_box = nanbox_pointer_inline(ctx.block(), &new_handle);
                emit_grow_mutator_writeback(ctx, *array_id, &new_box)?;
            }
            let len_i32 = ctx
                .block()
                .call(I32, "js_array_length", &[(I64, &new_handle)]);
            Ok(ctx.block().sitofp(I32, &len_i32, DOUBLE))
        }
        // Issue #655 (chained-receiver path): without this arm, a
        // chained `obj.field.splice(...)` resolved through `is_array_expr`
        // (now that interface property types are recognized) but fell
        // off the end of `lower_array_method` into the silent fallback,
        // which returned the receiver unchanged and never invoked
        // `js_array_splice`. The HIR-level `Expr::ArraySplice` variant
        // covers single-identifier receivers; this arm handles the
        // generic property-chained case (`m.get(k)!.field.splice(...)`).
        // Writeback to the source storage is best-effort for the local
        // / module-global cases — when the array's parent is a heap
        // property we do not re-emit a PropertySet here, matching the
        // existing `Expr::ArraySplice` lowering's tolerance for
        // missing local IDs. In-place mutation is correct regardless;
        // only growth-induced reallocation could leave the parent
        // pointing at the old header (queue-deletion patterns never
        // grow the array, so the issue's hot path is unaffected).
        "splice" => {
            let start_d = if args.is_empty() {
                "0.0".to_string()
            } else {
                lower_expr(ctx, &args[0])?
            };
            let count_d = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else if args.is_empty() {
                "0.0".to_string()
            } else {
                "2147483647.0".to_string()
            };
            let mut item_vals: Vec<String> = Vec::new();
            for it in args.iter().skip(2) {
                item_vals.push(lower_expr(ctx, it)?);
            }
            let blk = ctx.block();
            let out_slot = blk.alloca(I64);
            blk.store(I64, "0", &out_slot);
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // ToIntegerOrInfinity via the clamping helper: `fptosi` on
            // ±Infinity/NaN is LLVM poison — `splice(Infinity, 3)` deleted
            // from index 0 (test262 splice/S15.4.4.12_A2.1_T3). The helper
            // clamps +Inf → i32::MAX (→ len downstream), -Inf → i32::MIN
            // (→ 0 after relative-index resolution), NaN → 0.
            let start_i32 = blk.call(I32, "js_array_splice_delete_count", &[(DOUBLE, &start_d)]);
            let count_i32 = blk.call(I32, "js_array_splice_delete_count", &[(DOUBLE, &count_d)]);
            let (items_ptr, items_count_str) = if item_vals.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = item_vals.len();
                let buf_reg = blk.next_reg();
                blk.emit_raw(format!("{} = alloca [{} x double]", buf_reg, n));
                for (i, val) in item_vals.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, val, &slot);
                }
                (buf_reg, format!("{}", n))
            };
            let deleted_handle = blk.call(
                I64,
                "js_array_splice",
                &[
                    (I64, &recv_handle),
                    (I32, &start_i32),
                    (I32, &count_i32),
                    (PTR, &items_ptr),
                    (I32, &items_count_str),
                    (PTR, &out_slot),
                ],
            );
            // Best-effort writeback when the receiver is a single local
            // — mirrors the `Expr::ArraySplice` lowering. For
            // property-chained receivers we leave the parent pointing
            // at the original header; growth-induced reallocation is
            // a corner case that doesn't fire on queue-shrink usage.
            if let Expr::LocalGet(array_id) = object {
                let modified_handle = ctx.block().load(I64, &out_slot);
                let modified_box = nanbox_pointer_inline(ctx.block(), &modified_handle);
                if let Some(slot) = ctx.locals.get(array_id).cloned() {
                    ctx.block().store(DOUBLE, &modified_box, &slot);
                } else if let Some(global_name) = ctx.module_globals.get(array_id).cloned() {
                    let g_ref = format!("@{}", global_name);
                    emit_root_nanbox_store_on_block(ctx.block(), &modified_box, &g_ref);
                }
            }
            Ok(nanbox_pointer_inline(ctx.block(), &deleted_handle))
        }
        // #2384: build a real `.next()`-bearing iterator OBJECT (not an eager
        // materialized array) so manual `.next().value` matches Node; spread /
        // for-of / Array.from already drive `.next()` on the iterator class id.
        "entries" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            // Full bits, not the 48-bit mask: the runtime router classifies
            // non-pointer receivers (Web Streams handle ids are plain
            // doubles whose masked bits look like heap addresses).
            let recv_handle = blk.bitcast_double_to_i64(&recv_box);
            let result = blk.call(I64, "js_array_entries_iter_obj", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "keys" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = blk.bitcast_double_to_i64(&recv_box);
            let result = blk.call(I64, "js_array_keys_iter_obj", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "values" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = blk.bitcast_double_to_i64(&recv_box);
            let result = blk.call(I64, "js_array_values_iter_obj", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // Issue #515 followup: `arr.with(idx, val)` reaches here when the
        // receiver passes `is_array_expr` but the HIR fold bailed (e.g. the
        // receiver is an `any`-typed local whose initializer is an Array
        // literal — `is_array_expr` recognizes this even though the binding's
        // declared type is `Type::Any`). Without this arm the catch-all below
        // silently returned the receiver unchanged.
        "with" if args.len() >= 2 => {
            let idx_d = lower_expr(ctx, &args[0])?;
            let val_d = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_array_with",
                &[(I64, &recv_handle), (DOUBLE, &idx_d), (DOUBLE, &val_d)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // #2384: iterator-protocol methods. A value-level `arr.entries()` /
        // `.keys()` / `.values()` now yields a real iterator OBJECT, but
        // `is_array_expr` still classifies the binding as an array (the static
        // type is `Array<…>`), so `e.next()` routes here. Returning `recv_box`
        // (the old catch-all) handed back the iterator object itself —
        // `.next().value` was then `undefined` (same class of bug as #800's
        // `lastIndexOf`). Route through the runtime's generic dispatch so the
        // `ARRAY_ITERATOR_CLASS_ID` check reaches `dispatch_array_iterator_method`.
        // #2808: `Array.prototype.toLocaleString` has no static HIR fold, so a
        // typed / `as any` array receiver reaches this codegen path. The old
        // catch-all returned the receiver unchanged (so `JSON.stringify` saw
        // the array, not the joined locale string). Route through the runtime
        // dispatch tower, which walks elements and calls each element's own
        // `toLocaleString(locales, options)`.
        //
        // #2803 defensive: `toReversed` / `toSorted` / `toSpliced` normally fold
        // to dedicated `Expr::ArrayTo*` nodes upstream, but if that fold ever
        // bails for an `any`-typed receiver they would otherwise hit the
        // receiver-returning catch-all below. Dispatching them dynamically here
        // keeps the immutable-copy semantics (the runtime arms added in #2803).
        "next" | "return" | "throw" | "toLocaleString" | "toReversed" | "toSorted"
        | "toSpliced" => {
            let mut lowered_args = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len_str = entry.byte_len.to_string();
            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a_val) in lowered_args.iter().enumerate() {
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
            let result = ctx.block().call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len),
                ],
            );
            Ok(result)
        }
        // #3148: TypedArray.prototype.set(source, offset?). Copies elements
        // from an Array/TypedArray source into this typed array. The runtime
        // helper no-ops for non-typed-array receivers, so it is safe under the
        // broadened `is_array_expr` (which also routes plain arrays here).
        "set" => {
            let src_box = if let Some(a) = args.first() {
                lower_expr(ctx, a)?
            } else {
                double_literal(f64::from_bits(TAG_UNDEFINED))
            };
            let off_box = if args.len() >= 2 {
                lower_expr(ctx, &args[1])?
            } else {
                double_literal(0.0)
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            Ok(blk.call(
                DOUBLE,
                "js_typed_array_set_from",
                &[(I64, &recv_handle), (DOUBLE, &src_box), (DOUBLE, &off_box)],
            ))
        }
        // #3148: TypedArray.prototype.subarray(begin?, end?) — returns a new
        // same-kind TypedArray over the selected range.
        "subarray" => {
            let (has_begin, begin_box) = if let Some(a) = args.first() {
                ("1".to_string(), lower_expr(ctx, a)?)
            } else {
                ("0".to_string(), double_literal(0.0))
            };
            let (has_end, end_box) = if args.len() >= 2 {
                ("1".to_string(), lower_expr(ctx, &args[1])?)
            } else {
                ("0".to_string(), double_literal(0.0))
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_typed_array_subarray",
                &[
                    (I64, &recv_handle),
                    (I32, &has_begin),
                    (DOUBLE, &begin_box),
                    (I32, &has_end),
                    (DOUBLE, &end_box),
                ],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // Unknown method on an (statically-typed) array receiver. This is NOT
        // a built-in array method, so it is a user-stored own property
        // (`arr.getClass = Object.prototype.toString; arr.getClass()`,
        // `arr.myFn = function(){…}; arr.myFn()`) or an inherited method. The
        // old catch-all returned `recv_box` unchanged, so any such call
        // silently evaluated to the array itself. Route through the runtime
        // dispatch tower, which checks the ARRAY_NAMED_PROPS side table for a
        // stored callable (invoking it with `this` = arr) before falling back.
        _ => {
            let mut lowered_args = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len_str = entry.byte_len.to_string();
            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a_val) in lowered_args.iter().enumerate() {
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
            let result = ctx.block().call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len),
                ],
            );
            Ok(result)
        }
    }
}
