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
    lower_expr, nanbox_pointer_inline, nanbox_string_inline, unbox_str_handle, unbox_to_i64, FnCtx,
};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

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
            if !args.is_empty() {
                bail!("perry-codegen: Array.pop takes no args, got {}", args.len());
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Returns f64 directly (the popped element, NaN if empty).
            Ok(blk.call(DOUBLE, "js_array_pop_f64", &[(I64, &recv_handle)]))
        }
        "join" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.join expects 1 arg (separator), got {}",
                    args.len()
                );
            }
            let sep_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Separator is a JS string. `unbox_str_handle` materializes SSO
            // values to a real heap StringHeader; plain `unbox_to_i64`
            // returns the inline-payload bits, which `js_array_join` then
            // dereferences as a `*StringHeader` and reads `byte_len` from
            // — producing garbage / silent empty join. Same SSO bug class
            // as #214.
            let sep_handle = unbox_str_handle(blk, &sep_box);
            let result_handle = blk.call(
                I64,
                "js_array_join",
                &[(I64, &recv_handle), (I64, &sep_handle)],
            );
            Ok(nanbox_string_inline(blk, &result_handle))
        }
        "some" | "every" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.{} expects 1 arg, got {}",
                    property,
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            // arr.concat(other) — call js_array_concat (already declared).
            // For simplicity we only handle single-argument concat.
            if args.len() != 1 {
                return Ok(recv_box);
            }
            let other_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let other_handle = unbox_to_i64(blk, &other_box);
            let result = blk.call(
                I64,
                "js_array_concat",
                &[(I64, &recv_handle), (I64, &other_handle)],
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
                let cb_handle = unbox_to_i64(blk, &cb_box);
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
        "flat" => {
            // arr.flat() / arr.flat(depth) — depth is ignored for now.
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_flat", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "flatMap" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.flatMap expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.find expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            Ok(blk.call(
                DOUBLE,
                "js_array_find",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            ))
        }
        "findIndex" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.findIndex expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let i32_v = blk.call(
                I32,
                "js_array_findIndex",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "findLast" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.findLast expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            Ok(blk.call(
                DOUBLE,
                "js_array_find_last",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            ))
        }
        "findLastIndex" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.findLastIndex expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let i32_v = blk.call(
                I32,
                "js_array_find_last_index",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "reduce" => {
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: Array.reduce expects 1-2 args, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let (has_initial, initial_box) = if args.len() == 2 {
                let init = lower_expr(ctx, &args[1])?;
                (1i32, init)
            } else {
                (0i32, "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            if args.is_empty() || args.len() > 2 {
                bail!(
                    "perry-codegen: Array.reduceRight expects 1-2 args, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let (has_initial, initial_box) = if args.len() == 2 {
                let init = lower_expr(ctx, &args[1])?;
                (1i32, init)
            } else {
                (0i32, "0.0".to_string())
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
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
            if args.len() != 1 {
                bail!("perry-codegen: Array.map expects 1 arg, got {}", args.len());
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(
                I64,
                "js_array_map",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "filter" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.filter expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            let result = blk.call(
                I64,
                "js_array_filter",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "forEach" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.forEach expects 1 arg, got {}",
                    args.len()
                );
            }
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void(
                "js_array_forEach",
                &[(I64, &recv_handle), (I64, &cb_handle)],
            );
            // forEach returns undefined
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        "includes" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.includes expects 1 arg, got {}",
                    args.len()
                );
            }
            let val_box = lower_expr(ctx, &args[0])?;
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
                &[(I64, &recv_handle), (DOUBLE, &val_box)],
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
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.indexOf expects 1 arg, got {}",
                    args.len()
                );
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            // Issue #214: route through `_jsvalue` so string elements
            // match by content (handles SSO + heap-string mixed arrays
            // — `arr.indexOf("hello")` on a `JSON.parse(...)`-derived
            // string array returned -1 because the SSO element bits
            // never bit-equal the heap-string needle bits). Mirrors
            // the existing `includes` arm.
            let i32_v = blk.call(
                I32,
                "js_array_indexOf_jsvalue",
                &[(I64, &recv_handle), (DOUBLE, &val_box)],
            );
            Ok(blk.sitofp(I32, &i32_v, DOUBLE))
        }
        "at" => {
            if args.len() != 1 {
                bail!("perry-codegen: Array.at expects 1 arg, got {}", args.len());
            }
            let idx_box = lower_expr(ctx, &args[0])?;
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
            // Zero-arg `.slice()` is the JS shallow-copy idiom: same as
            // `.slice(0)`. Lower it to start=0, end=i32::MAX.
            let start_i32 = if args.is_empty() {
                "0".to_string()
            } else {
                let start_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                blk.fptosi(DOUBLE, &start_box, I32)
            };
            let end_i32 = if args.len() == 2 {
                let end_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                blk.fptosi(DOUBLE, &end_box, I32)
            } else {
                "2147483647".to_string()
            };
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_array_slice",
                &[(I64, &recv_handle), (I32, &start_i32), (I32, &end_i32)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "shift" => {
            if !args.is_empty() {
                bail!(
                    "perry-codegen: Array.shift takes no args, got {}",
                    args.len()
                );
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            Ok(blk.call(DOUBLE, "js_array_shift_f64", &[(I64, &recv_handle)]))
        }
        "fill" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.fill expects 1 arg, got {}",
                    args.len()
                );
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_array_fill",
                &[(I64, &recv_handle), (DOUBLE, &val_box)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "unshift" => {
            if args.len() != 1 {
                bail!(
                    "perry-codegen: Array.unshift expects 1 arg, got {}",
                    args.len()
                );
            }
            let val_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(
                I64,
                "js_array_unshift_f64",
                &[(I64, &recv_handle), (DOUBLE, &val_box)],
            );
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "entries" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_entries", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "keys" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_keys", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        "values" => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let result = blk.call(I64, "js_array_values", &[(I64, &recv_handle)]);
            Ok(nanbox_pointer_inline(blk, &result))
        }
        // Best-effort fallback: lower args for side effects, return
        // the receiver.
        _ => {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(recv_box)
        }
    }
}
