//! The generated event loop's liveness predicate, split out of `entry.rs` when
//! turnloop P3's phase-ordered body pushed that file over the 2,000-line cap.

use crate::expr::FnCtx;
use crate::types::I32;

/// Emit the module's entry function.
///
/// For the **entry module**: emits `int main()` that bootstraps GC, runs
/// the entry module's own string pool init, then calls every non-entry
/// module's `<prefix>__init` function in order, then runs the entry
/// module's top-level statements, then `return 0`.
///
/// #5579: emit the global-object reflection of a Script's bare top-level
/// `function` declarations (`globalThis[name] = <fn>`). Called from the
/// entry-module branch only for non-ESM programs, before user init runs.
///
/// Each name is reflected with a heap closure built exactly as `Expr::FuncRef`
/// does (`js_closure_alloc_singleton(@__perry_wrap_<sym>)`), so the property
/// value is callable and `typeof globalThis[name] === "function"`. The
/// `hir.script_global_functions` list is already deduped (last declaration
/// wins) and excludes nested closures / object-literal methods, which must
/// not pollute the global object.
/// #9441 — emit the event loop's "is any source still live?" test into the
/// current block and return the i32 disjunction.
///
/// Emitted TWICE per loop: once in `event_loop.check_pending`, which decides
/// whether to run the body, and once in `event_loop.body_check`, which decides
/// whether the body's park is worth taking. It is one function so the two
/// cannot drift — an arm added to the header but not to the post-body check
/// would silently restore the second of idle latency this exists to remove.
pub(super) fn emit_event_loop_liveness(ctx: &mut FnCtx<'_>, needs_stdlib: bool) -> String {
    let has_timers = ctx.block().call(I32, "js_timer_has_pending", &[]);
    let has_callbacks = ctx.block().call(I32, "js_callback_timer_has_pending", &[]);
    let has_intervals = ctx.block().call(I32, "js_interval_timer_has_pending", &[]);
    // Cron jobs (node-cron schedule() / npm cron's CronJob). Guarded on
    // `needs_stdlib` like `js_stdlib_init_dispatch` — the runtime-only link
    // doesn't carry the cron symbols (and a cron import always pulls stdlib
    // in). With stdlib linked the symbol always resolves: perry-ext-cron or
    // the bundled scheduler provide the real queue; perry-stdlib exports a
    // 0-returning stub otherwise. Without this gate (and the tick in
    // loop_body) a program whose only live work is a running cron job exits
    // immediately and scheduled callbacks never fire.
    let has_cron = if needs_stdlib {
        ctx.block().call(I32, "js_cron_timer_has_pending", &[])
    } else {
        "0".to_string()
    };
    let has_stdlib = ctx.block().call(I32, "js_stdlib_has_active_handles", &[]);
    let has_ffi_callbacks =
        ctx.block()
            .call(I32, "js_bun_ffi_has_active_threadsafe_callbacks", &[]);
    // #591: TASK_QUEUE may carry a pending `.then` continuation that was
    // queued by `js_run_stdlib_pump`'s resolution path in the SAME body
    // iteration that already drained the inflight counter and
    // PENDING_RESOLUTIONS to zero. Without this gate, the header check would
    // flip to "exit" before the next body's microtask drain ran the
    // continuation.
    let has_microtasks = ctx.block().call(I32, "js_microtasks_pending", &[]);
    let any1 = ctx.block().or(I32, &has_timers, &has_callbacks);
    let any2 = ctx.block().or(I32, &has_intervals, &has_stdlib);
    let any2 = ctx.block().or(I32, &any2, &has_ffi_callbacks);
    let any3 = ctx.block().or(I32, &any1, &any2);
    let any4 = ctx.block().or(I32, &any3, &has_cron);
    ctx.block().or(I32, &any4, &has_microtasks)
}
