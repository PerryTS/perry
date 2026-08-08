//! Timer lowerings for `Expr::ExternFuncRef` — `setTimeout`, `setInterval`,
//! `setImmediate` and their `clear*` siblings.
//!
//! Split out of `extern_func.rs` (#7210) when that file crossed the 2000-line
//! cap. Pure mechanical move: every arm body below is a verbatim copy of the
//! arm it replaced, reached from `try_lower_extern_func_call`'s dispatch.
//!
//! The three trailing-argument forms share one GC contract, documented on the
//! `setTimeout` arm: the whole argument list is lowered through
//! [`crate::rooting::with_operands_rooted`], and only then stored into the
//! stack buffer.
//!
//! # Layer 1 migration (#7615)
//!
//! Every rooting decision in this file goes through `crate::rooting`. The three
//! `lower_exprs_rooted` + `temp_root_release` pairs became
//! [`with_operands_rooted`], which owns the release on every path out instead of
//! handing it back as a guard the arm has to remember to drop.
//!
//! **The migration found a live bug, and it is the reason this file was worth
//! migrating rather than merely relisting.** #7210 rooted the *trailing-argument*
//! forms of `setTimeout`/`setInterval` and left their **two-argument** siblings
//! alone, even though the comment it wrote names the exact window those siblings
//! have:
//!
//! ```text
//!   %r6 = call i64 @js_closure_alloc(...)          ; the callback
//!   %r8 = bitcast i64 %r7 to double                ; cb_box -- a BARE register
//!   %r9 = call double @perry_fn_mod__churn()       ; the DELAY. User code.
//!   %r10 = call i64 @js_timer_validate_callback(double %r8, i32 0)   ; stale
//! ```
//!
//! `setTimeout(() => …, churn())` is legal JS — the delay is an arbitrary
//! expression — so `%r8` crosses a real user call with back-edge polls while
//! nothing roots it. Under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1` the
//! baseline throws #7210's own symptom text, `The "callback" argument must be of
//! type function. Received an instance of Object`, where node prints the
//! scheduled timer.
//!
//! The one-argument and `clear*` arms are deliberately NOT routed through the
//! API. Their operand is consumed by the very next emission, so the window is
//! empty — counted in EMISSIONS, not in source lines — and
//! `with_operands_rooted` over a one-element list provably emits nothing
//! (`any_may_trigger_gc` over an empty tail is `false`). Routing them through it
//! would buy uniformity and no protection.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::rooting::with_operands_rooted;
use crate::types::{DOUBLE, I32, I64, PTR};

/// Fill an entry-block `[n x double]` staging buffer from `vals`, and yield the
/// `ptr` to its first element.
///
/// Shared by the four trailing-argument arms. The buffer is NOT a GC root —
/// nothing in the precise walk visits an `alloca_entry_array` — so every caller
/// fills it from values that have already been re-read below the last collection
/// point, and emits nothing that can collect between the fill and the consuming
/// call.
pub(super) fn fill_arg_buffer(ctx: &mut FnCtx<'_>, vals: &[String]) -> String {
    let n = vals.len();
    let buf = ctx.func.alloca_entry_array(DOUBLE, n);
    for (i, v) in vals.iter().enumerate() {
        let blk = ctx.block();
        let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
        blk.store(DOUBLE, v, &slot);
    }
    let ptr_reg = ctx.block().next_reg();
    ctx.block().emit_raw(format!(
        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
        ptr_reg, n, buf
    ));
    ptr_reg
}

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
        // The delay is an arbitrary expression, so lowering it is a collection
        // point sitting between the callback's allocation and the
        // `js_timer_validate_callback` that reads it — see the module header for
        // the emitted IR and the reproducer. `setTimeout(fn, 100)` emits exactly
        // the IR it emitted before: a literal delay cannot collect, so
        // `operand_protection` routes the callback to `Reuse` and nothing is
        // pushed.
        "setTimeout" if args.len() == 2 => {
            let boxed = with_operands_rooted(ctx, &[&args[0], &args[1]], |ctx, vals| {
                let (cb_box, delay_box) = (vals[0].clone(), vals[1].clone());
                let blk = ctx.block();
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, "0")],
                );
                let id = blk.call(
                    I64,
                    "js_set_timeout_callback",
                    &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                );
                Ok(nanbox_pointer_inline(blk, &id))
            })?;
            return Ok(Some(boxed));
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
            let n = args.len() - 1;
            let boxed = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                let cb_box = vals[0].clone();
                let ptr_reg = fill_arg_buffer(ctx, &vals[1..]);
                let blk = ctx.block();
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, "2")],
                );
                let id = blk.call(
                    I64,
                    "js_set_immediate_callback_args",
                    &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
                );
                Ok(nanbox_pointer_inline(blk, &id))
            })?;
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
            // `with_operands_rooted` closes both at once: it protects each value
            // as soon as it is produced and re-reads them all below the last
            // one, so the stores below observe post-collection addresses. Cost
            // is zero when nothing in the list can collect (`OperandProtection::
            // Reuse`), which is the `setTimeout(fn, 0, someLocal)` case. The
            // release happens after `body` returns — below the consuming call,
            // which reads the buffer — and on the error path too.
            let arg_refs: Vec<&Expr> = args.iter().collect();
            let n = args.len() - 2;
            let boxed = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                let (cb_box, delay_box) = (vals[0].clone(), vals[1].clone());
                let ptr_reg = fill_arg_buffer(ctx, &vals[2..]);
                let blk = ctx.block();
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, "0")],
                );
                let id = blk.call(
                    I64,
                    "js_set_timeout_callback_args",
                    &[
                        (I64, &cb_handle),
                        (DOUBLE, &delay_box),
                        (PTR, &ptr_reg),
                        (I32, &n.to_string()),
                    ],
                );
                Ok(nanbox_pointer_inline(blk, &id))
            })?;
            return Ok(Some(boxed));
        }
        // The 2-arg twin of the `setTimeout` arm above, and it carried the same
        // live bug — see the module header.
        "setInterval" if args.len() == 2 => {
            let boxed = with_operands_rooted(ctx, &[&args[0], &args[1]], |ctx, vals| {
                let (cb_box, delay_box) = (vals[0].clone(), vals[1].clone());
                let blk = ctx.block();
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, "1")],
                );
                let id = blk.call(
                    I64,
                    "setInterval",
                    &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                );
                Ok(nanbox_pointer_inline(blk, &id))
            })?;
            return Ok(Some(boxed));
        }
        "setInterval" if args.len() >= 3 => {
            // #7210: same treatment as `setTimeout` above.
            let arg_refs: Vec<&Expr> = args.iter().collect();
            let n = args.len() - 2;
            let boxed = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                let (cb_box, delay_box) = (vals[0].clone(), vals[1].clone());
                let ptr_reg = fill_arg_buffer(ctx, &vals[2..]);
                let blk = ctx.block();
                let cb_handle = blk.call(
                    I64,
                    "js_timer_validate_callback",
                    &[(DOUBLE, &cb_box), (I32, "1")],
                );
                let id = blk.call(
                    I64,
                    "js_set_interval_callback_args",
                    &[
                        (I64, &cb_handle),
                        (DOUBLE, &delay_box),
                        (PTR, &ptr_reg),
                        (I32, &n.to_string()),
                    ],
                );
                Ok(nanbox_pointer_inline(blk, &id))
            })?;
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
