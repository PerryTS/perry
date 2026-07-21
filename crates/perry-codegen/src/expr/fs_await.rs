//! FsUnlinkSync + Await.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I32, I64};

use super::{lower_expr, unbox_to_i64, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::FsUnlinkSync(path) => {
            let p = lower_expr(ctx, path)?;
            let _ = ctx.block().call(I32, "js_fs_unlink_sync", &[(DOUBLE, &p)]);
            Ok(double_literal(0.0))
        }

        // -------- Await with busy-wait loop --------
        //
        // Structure:
        //
        //   <current>:
        //     %promise = unbox(<inner>)
        //     br check
        //   check:
        //     %state = call js_promise_state(%promise)  ; 0=pending,1=fulfilled,2=rejected
        //     %is_pending = icmp eq %state, 0
        //     br i1 %is_pending, wait, settled
        //   wait:
        //     call js_promise_run_microtasks()
        //     call js_stdlib_process_pending()
        //     call js_wait_for_event()      ; condvar wait, issue #84
        //     br check
        //   settled:
        //     %state2 = call js_promise_state(%promise)
        //     %is_rejected = icmp eq %state2, 2
        //     br i1 %is_rejected, reject, done
        //   reject:
        //     %reason = call js_promise_reason(%promise)
        //     call js_throw(%reason)  ; void; never returns
        //     unreachable
        //   done:
        //     %value = call js_promise_value(%promise)
        //
        // Returns %value as a NaN-boxed double.
        Expr::Await(operand) => {
            let raw_operand = lower_expr(ctx, operand)?;

            // Issue #586: ECMAScript thenable assimilation. Before the
            // is-Promise branch, route the operand through
            // `js_assimilate_thenable`. For real Promises and non-thenable
            // values it's a passthrough; for objects whose class chain
            // defines `.then`, it allocates a wrapper Promise, invokes
            // `value.then(resolve, reject)`, and returns the wrapper —
            // which the existing polling loop below then drives until
            // resolve/reject fires. Without this, `await thenable` would
            // fall through to the merge block with the thenable itself
            // and the await would resolve with the object instead of
            // calling its `.then` (drizzle-orm's `QueryPromise.execute()`
            // never ran for `await db.select().from(users)`).
            let assimilated_box =
                ctx.block()
                    .call(DOUBLE, "js_assimilate_thenable", &[(DOUBLE, &raw_operand)]);
            // V8 fallback promises cross into native code as JS_HANDLE_TAG
            // values. Route the value through the registered adapter before
            // the native Promise check so await polls a real Perry Promise.
            let promise_box = ctx.block().call(
                DOUBLE,
                "js_await_any_promise",
                &[(DOUBLE, &assimilated_box)],
            );

            // Defensive guard: if the operand is not actually a
            // Promise (e.g. `await someNumber` or an unsupported
            // runtime function that returned a raw handle), fall
            // back to JS semantics — "await non-promise returns
            // the value itself" — instead of unboxing garbage bits
            // and polling `js_promise_state` on a random pointer.
            //
            // We call `js_value_is_promise(f64) -> i32` (GC-type
            // check) and branch: truthy → existing polling path,
            // falsy → store the box into a result slot and jump
            // straight to the merge block.
            //
            // The result is materialized via an `alloca` slot so the
            // merge block can reload a single SSA value without
            // having to thread explicit phi nodes through every
            // intermediate block. Hoisted to the entry block so the
            // slot dominates the merge block even when this Await is
            // itself nested inside an if-arm.
            let result_slot = ctx.func.alloca_entry(DOUBLE);
            // Pre-seed with the boxed operand so the non-promise
            // branch just needs to jump to merge.
            ctx.block().store(DOUBLE, &promise_box, &result_slot);

            let is_promise_i32 =
                ctx.block()
                    .call(I32, "js_value_is_promise", &[(DOUBLE, &promise_box)]);
            let is_promise_bool = ctx.block().icmp_ne(I32, &is_promise_i32, "0");

            let drain_once_idx = ctx.new_block("await.drain_once");
            let check_idx = ctx.new_block("await.check");
            let wait_idx = ctx.new_block("await.wait");
            let settled_idx = ctx.new_block("await.settled");
            let reject_idx = ctx.new_block("await.reject");
            let done_idx = ctx.new_block("await.done");
            let merge_idx = ctx.new_block("await.merge");

            let drain_once_label = ctx.block_label(drain_once_idx);
            let check_label = ctx.block_label(check_idx);
            let wait_label = ctx.block_label(wait_idx);
            let settled_label = ctx.block_label(settled_idx);
            let reject_label = ctx.block_label(reject_idx);
            let done_label = ctx.block_label(done_idx);
            let merge_label = ctx.block_label(merge_idx);

            ctx.block()
                .cond_br(&is_promise_bool, &drain_once_label, &merge_label);

            // === drain_once ===
            // Run pending promise/queueMicrotask jobs before the first state
            // check. When the promise is already settled (e.g.
            // `await Promise.resolve()`) the wait loop below is never
            // entered, so jobs queued before this await would never fire
            // before execution continues. Promise jobs ONLY — nextTick
            // callbacks queued in the same synchronous stretch wait for the
            // next real tick boundary, matching Node's checkpoint ordering
            // (#788; previously this drained the tick queue instead, so a
            // nextTick callback overtook earlier-queued microtasks). The
            // wait loop covers ticks/timers for pending promises.
            ctx.current_block = drain_once_idx;
            let _ = ctx.block().call(I32, "js_promise_run_promise_jobs", &[]);
            ctx.block().br(&check_label);

            // === check ===
            // Unbox the promise in each block that uses it — LLVM's
            // SSA form requires every value definition to dominate
            // its uses, and there's no single predecessor block we
            // could hoist the unbox into (check is reachable from
            // both the initial branch AND from `wait`).
            ctx.current_block = check_idx;
            let promise_handle = unbox_to_i64(ctx.block(), &promise_box);
            let state = ctx
                .block()
                .call(I32, "js_promise_state", &[(I64, &promise_handle)]);
            let is_pending = ctx.block().icmp_eq(I32, &state, "0");
            ctx.block()
                .cond_br(&is_pending, &wait_label, &settled_label);

            // === wait ===
            // Drive microtasks AND pending timers on each tick so that
            // `await new Promise(r => setTimeout(r, 1))` and similar
            // patterns eventually resolve. Without the timer ticks the
            // await loop busy-waits forever.
            ctx.current_block = wait_idx;
            ctx.block()
                .call_void("js_promise_run_microtasks_await_loop", &[]);
            // Drain the stdlib's tokio async queue — fetch, database
            // queries, and other async stdlib operations queue their
            // results via queue_promise_resolution and need this pump
            // to actually resolve the promises on the main thread.
            ctx.block().call_void("js_run_stdlib_pump", &[]);
            // #5437: tick through the await-loop entry, which suspends the
            // timer dispatch guard — a busy-wait await inside a timer /
            // setImmediate callback (every HTTP request handler) must still
            // fire due timers or a setImmediate-scheduled resolution (React's
            // server renderer) can never settle.
            let _ = ctx.block().call(I32, "js_await_loop_tick_timers", &[]);

            if !ctx.is_async_fn {
                let wait_for_event_idx = ctx.new_block("await.wait_for_event");
                let unsettled_exit_idx = ctx.new_block("await.unsettled_exit");
                let wait_for_event_label = ctx.block_label(wait_for_event_idx);
                let unsettled_exit_label = ctx.block_label(unsettled_exit_idx);

                let promise_handle_wait = unbox_to_i64(ctx.block(), &promise_box);
                let state_after_tick =
                    ctx.block()
                        .call(I32, "js_promise_state", &[(I64, &promise_handle_wait)]);
                let still_pending = ctx.block().icmp_eq(I32, &state_after_tick, "0");
                let has_timers = ctx.block().call(I32, "js_timer_has_pending", &[]);
                let has_callbacks = ctx.block().call(I32, "js_callback_timer_has_pending", &[]);
                let has_intervals = ctx.block().call(I32, "js_interval_timer_has_pending", &[]);
                let has_stdlib = ctx.block().call(I32, "js_stdlib_has_active_handles", &[]);
                let has_microtasks = ctx.block().call(I32, "js_microtasks_pending", &[]);
                let any1 = ctx.block().or(I32, &has_timers, &has_callbacks);
                let any2 = ctx.block().or(I32, &has_intervals, &has_stdlib);
                let any3 = ctx.block().or(I32, &any1, &any2);
                let any = ctx.block().or(I32, &any3, &has_microtasks);
                let no_refed_work = ctx.block().icmp_eq(I32, &any, "0");
                let should_exit = ctx.block().and(I1, &still_pending, &no_refed_work);
                ctx.block()
                    .cond_br(&should_exit, &unsettled_exit_label, &wait_for_event_label);

                ctx.current_block = unsettled_exit_idx;
                ctx.block()
                    .call_void("js_unsettled_top_level_await_exit", &[]);
                ctx.block().unreachable();

                ctx.current_block = wait_for_event_idx;
            }

            // Issue #84: condvar wait — wakes the instant the awaited
            // promise's resolver (or any other tokio queue push) calls
            // js_notify_main_thread, instead of paying the old 1 ms
            // hard-sleep quantum per await iteration.
            ctx.block().call_void("js_wait_for_event", &[]);
            ctx.block().br(&check_label);

            // === settled ===
            ctx.current_block = settled_idx;
            let promise_handle2 = unbox_to_i64(ctx.block(), &promise_box);
            let state2 = ctx
                .block()
                .call(I32, "js_promise_state", &[(I64, &promise_handle2)]);
            let is_rejected = ctx.block().icmp_eq(I32, &state2, "2");
            ctx.block()
                .cond_br(&is_rejected, &reject_label, &done_label);

            // === reject ===
            // Same spec-corner as `Stmt::Throw`: inside an async function
            // with no enclosing user try-frame, an awaited rejection must
            // settle the caller's promise as rejected — not unwind. Without
            // this, `async function f() { await Promise.reject(e); }`
            // would terminate the process because `js_throw` longjmps
            // through a non-existent setjmp frame.
            ctx.current_block = reject_idx;
            let promise_handle3 = unbox_to_i64(ctx.block(), &promise_box);
            let reason = ctx
                .block()
                .call(DOUBLE, "js_promise_reason", &[(I64, &promise_handle3)]);
            if ctx.is_async_fn && ctx.try_depth == 0 {
                let blk = ctx.block();
                let handle = blk.call(I64, "js_promise_rejected", &[(DOUBLE, &reason)]);
                let boxed = crate::expr::nanbox_pointer_inline_pub(blk, &handle);
                blk.ret(DOUBLE, &boxed);
            } else {
                ctx.block().call_void("js_throw", &[(DOUBLE, &reason)]);
                ctx.block().unreachable();
            }

            // === done ===
            ctx.current_block = done_idx;
            let promise_handle4 = unbox_to_i64(ctx.block(), &promise_box);
            let value = ctx
                .block()
                .call(DOUBLE, "js_promise_value", &[(I64, &promise_handle4)]);
            ctx.block().store(DOUBLE, &value, &result_slot);
            ctx.block().br(&merge_label);

            // === merge ===
            ctx.current_block = merge_idx;
            Ok(ctx.block().load(DOUBLE, &result_slot))
        }

        // -------- StaticFieldGet/Set --------
        // Look up the (class, field) → global symbol in the static
        // field registry built at compile_module time. Load/store
        // from the global directly. NativeModuleRef stays a stub.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
