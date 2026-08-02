//! Timer lowerings for `Expr::ExternFuncRef` — `setTimeout`, `setInterval`,
//! `setImmediate` and their `clear*` siblings.
//!
//! Split out of `extern_func.rs` (#7210) when that file crossed the 2000-line
//! cap. Pure mechanical move: every arm body below is a verbatim copy of the
//! arm it replaced, reached from `try_lower_extern_func_call`'s dispatch.
//!
//! The three trailing-argument forms share one GC contract, documented on the
//! `setTimeout` arm: the whole argument list is lowered through
//! `lower_exprs_rooted`, and only then stored into the stack buffer.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Lower a timer builtin, or `Ok(None)` if `name` is not one.
pub fn try_lower_extern_timer_call(
    ctx: &mut FnCtx<'_>,
    name: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    match name {
        // #1671: `setTimeout(fn)` with no explicit delay. Node treats a
        // missing/undefined delay as 0 (fires on the next timer tick).
        // Without this arm a 1-arg `setTimeout` falls through to the
        // catch-all below, which emits a bare LLVM call to `@setTimeout`
        // and the linker fails with `Undefined symbols: _setTimeout`
        // (hit by hono/jsx's `hooks/index.js`, which schedules a re-render
        // via `setTimeout(() => { … })`). Route it to the same runtime
        // entry as the 2-arg form with a zero delay.
        "setTimeout" if args.len() == 1 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            // #2013 — validate the callback type before unboxing the
            // pointer. `js_timer_validate_callback` throws
            // ERR_INVALID_ARG_TYPE for any non-callable value and
            // returns the raw closure pointer otherwise; the second
            // arg `0` is the type-name index for "setTimeout".
            let zero_idx = "0";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, zero_idx)],
            );
            let zero = double_literal(0.0);
            let id = blk.call(
                I64,
                "js_set_timeout_callback",
                &[(I64, &cb_handle), (DOUBLE, &zero)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setTimeout" if args.len() == 2 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let zero_idx = "0";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, zero_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_timeout_callback",
                &[(I64, &cb_handle), (DOUBLE, &delay_box)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setImmediate" if !args.is_empty() => {
            if args.len() == 1 {
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let two_idx = "2";
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, two_idx)],
                );
                let id = blk.call(I64, "js_set_immediate_callback", &[(I64, &cb_handle)]);
                return Ok(Some(nanbox_pointer_inline(blk, &id)));
            }

            // #7210: same treatment as `setTimeout` below — see the comment
            // there for why the callback register and the staging buffer are
            // one fix, not two.
            let arg_refs: Vec<&Expr> = args.iter().collect();
            let (vals, guard) = crate::expr::temp_root::lower_exprs_rooted(ctx, &arg_refs)?;
            let cb_box = vals[0].clone();
            let n = args.len() - 1;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, v) in vals.iter().skip(1).enumerate() {
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let two_idx = "2";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, two_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_immediate_callback_args",
                &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
            );
            let boxed = nanbox_pointer_inline(ctx.block(), &id);
            crate::expr::temp_root::temp_root_release(ctx, guard);
            return Ok(Some(boxed));
        }
        // Refs #665: `setTimeout(fn, delay, ...args)` — JS spec forwards
        // the trailing args to `fn` when the timer fires. Pack them into
        // a stack buffer of doubles and hand off to the varargs runtime
        // entry. Used by Promise-executor patterns like
        // `setTimeout(resolve, delay, res)` (rate-limiter-flexible's
        // `RateLimiterMemory.consume` is the discovering call site).
        "setTimeout" if args.len() >= 3 => {
            // #7210: lower the WHOLE argument list through `lower_exprs_rooted`,
            // then fill the buffer in a second, lowering-free pass.
            //
            // The previous shape had two unrooted windows, and the callback's
            // was the one that crashed. `cb_box` was lowered first and read at
            // `js_timer_validate_callback` — after `lower_expr(delay)` and after
            // every trailing argument's `lower_expr`. `setTimeout(cb, 0, {…},
            // churn())` therefore held a freshly-allocated closure in an SSA
            // register across a user call with loop back-edge polls, and the
            // moving minor inside `churn` left the register naming from-space:
            // `TypeError: The "callback" argument must be of type function.
            // Received an instance of Object`, deterministically, at base.
            //
            // The staging buffer is the second window and is the worse of the
            // two in kind: argument *i* sits in a bare `alloca_entry_array`,
            // which the precise root walk never visits, while argument *i+1* is
            // lowered. That is not staleness — nothing anywhere refers to the
            // object, so it is a premature SWEEP.
            //
            // `lower_exprs_rooted` closes both at once: it protects each value
            // as soon as it is produced and re-reads them all below the last
            // one, so the stores below observe post-collection addresses. Cost
            // is zero when nothing in the list can collect (`OperandProtection::
            // Reuse`), which is the `setTimeout(fn, 0, someLocal)` case.
            let arg_refs: Vec<&Expr> = args.iter().collect();
            let (vals, guard) = crate::expr::temp_root::lower_exprs_rooted(ctx, &arg_refs)?;
            let cb_box = vals[0].clone();
            let delay_box = vals[1].clone();
            let n = args.len() - 2;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, v) in vals.iter().skip(2).enumerate() {
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let zero_idx = "0";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, zero_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_timeout_callback_args",
                &[
                    (I64, &cb_handle),
                    (DOUBLE, &delay_box),
                    (crate::types::PTR, &ptr_reg),
                    (I32, &n.to_string()),
                ],
            );
            let boxed = nanbox_pointer_inline(ctx.block(), &id);
            // Released only after the consuming call: it reads the buffer.
            crate::expr::temp_root::temp_root_release(ctx, guard);
            return Ok(Some(boxed));
        }
        "setInterval" if args.len() == 2 => {
            let cb_box = lower_expr(ctx, &args[0])?;
            let delay_box = lower_expr(ctx, &args[1])?;
            let blk = ctx.block();
            let one_idx = "1";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, one_idx)],
            );
            let id = blk.call(
                I64,
                "setInterval",
                &[(I64, &cb_handle), (DOUBLE, &delay_box)],
            );
            return Ok(Some(nanbox_pointer_inline(blk, &id)));
        }
        "setInterval" if args.len() >= 3 => {
            // #7210: same treatment as `setTimeout` above.
            let arg_refs: Vec<&Expr> = args.iter().collect();
            let (vals, guard) = crate::expr::temp_root::lower_exprs_rooted(ctx, &arg_refs)?;
            let cb_box = vals[0].clone();
            let delay_box = vals[1].clone();
            let n = args.len() - 2;
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, v) in vals.iter().skip(2).enumerate() {
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let one_idx = "1";
            let cb_handle = blk.call(
                I64,
                "js_timer_validate_callback",
                &[(DOUBLE, &cb_box), (I32, one_idx)],
            );
            let id = blk.call(
                I64,
                "js_set_interval_callback_args",
                &[
                    (I64, &cb_handle),
                    (DOUBLE, &delay_box),
                    (crate::types::PTR, &ptr_reg),
                    (I32, &n.to_string()),
                ],
            );
            let boxed = nanbox_pointer_inline(ctx.block(), &id);
            crate::expr::temp_root::temp_root_release(ctx, guard);
            return Ok(Some(boxed));
        }
        "clearTimeout" if args.len() == 1 => {
            // Pass the raw NaN-boxed arg so the runtime accepts both the
            // handle and its primitive numeric id (`clearTimeout(+t)`, #1213).
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_timeout_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "clearInterval" if args.len() == 1 => {
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_interval_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        "clearImmediate" if args.len() == 1 => {
            let id_box = lower_expr(ctx, &args[0])?;
            ctx.block()
                .call_void("js_clear_immediate_value", &[(DOUBLE, &id_box)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
        _ => {}
    }
    Ok(None)
}
