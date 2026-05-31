//! node:perf_hooks codegen lowering — `performance.*` User Timing + ELU and
//! PerformanceObserver instance methods, dispatched to the `js_perf_*` runtime
//! helpers. Extracted from `native/mod.rs` to keep that file under the
//! 2k-LOC gate (`scripts/check_file_size.sh`).

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::double_literal;
use crate::types::DOUBLE;

/// Lower a `module == "perf_hooks"` NativeMethodCall. Returns `Ok(Some(value))`
/// when the (method, receiver) shape is handled, or `Ok(None)` to fall through
/// to the caller's remaining dispatch.
pub(super) fn lower_perf_hooks_method(
    ctx: &mut FnCtx<'_>,
    module: &str,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<Option<String>> {
    if module != "perf_hooks" {
        return Ok(None);
    }

    // PerformanceObserver instance methods. `obs.observe()` / `.disconnect()` /
    // `.takeRecords()` lower with `object = Some(obs)` (obs is typed as the
    // imported class); pass the observer object value through — the runtime
    // re-derives the registry index from it.
    if object.is_some() && matches!(method, "observe" | "disconnect" | "takeRecords") {
        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let obs_val = lower_expr(ctx, object.unwrap())?;
        let v = match method {
            "observe" => {
                let opts = match args.first() {
                    Some(e) => lower_expr(ctx, e)?,
                    None => undef,
                };
                ctx.block().call(
                    DOUBLE,
                    "js_perf_observer_observe",
                    &[(DOUBLE, &obs_val), (DOUBLE, &opts)],
                )
            }
            "disconnect" => {
                ctx.block()
                    .call(DOUBLE, "js_perf_observer_disconnect", &[(DOUBLE, &obs_val)])
            }
            _ => ctx.block().call(
                DOUBLE,
                "js_perf_observer_take_records",
                &[(DOUBLE, &obs_val)],
            ),
        };
        return Ok(Some(v));
    }

    // `performance.*` User Timing + ELU. Statically lowered (HIR
    // module_static.rs emits the receiver-less NativeMethodCall). All args are
    // NaN-boxed f64; absent trailing args pass `undefined`.
    if object.is_none() {
        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        let lower_or_undef = |ctx: &mut FnCtx<'_>, n: usize| -> Result<String> {
            match args.get(n) {
                Some(e) => lower_expr(ctx, e),
                None => Ok(undef.clone()),
            }
        };
        let v = match method {
            "now" => ctx.block().call(DOUBLE, "js_performance_now", &[]),
            "getEntries" => ctx.block().call(DOUBLE, "js_perf_get_entries", &[]),
            "mark" => {
                let name = lower_or_undef(ctx, 0)?;
                let opts = lower_or_undef(ctx, 1)?;
                ctx.block()
                    .call(DOUBLE, "js_perf_mark", &[(DOUBLE, &name), (DOUBLE, &opts)])
            }
            "measure" => {
                let a0 = lower_or_undef(ctx, 0)?;
                let a1 = lower_or_undef(ctx, 1)?;
                let a2 = lower_or_undef(ctx, 2)?;
                ctx.block().call(
                    DOUBLE,
                    "js_perf_measure",
                    &[(DOUBLE, &a0), (DOUBLE, &a1), (DOUBLE, &a2)],
                )
            }
            "getEntriesByType" => {
                let a0 = lower_or_undef(ctx, 0)?;
                ctx.block()
                    .call(DOUBLE, "js_perf_get_entries_by_type", &[(DOUBLE, &a0)])
            }
            "getEntriesByName" => {
                let a0 = lower_or_undef(ctx, 0)?;
                let a1 = lower_or_undef(ctx, 1)?;
                ctx.block().call(
                    DOUBLE,
                    "js_perf_get_entries_by_name",
                    &[(DOUBLE, &a0), (DOUBLE, &a1)],
                )
            }
            "clearMarks" => {
                let a0 = lower_or_undef(ctx, 0)?;
                ctx.block()
                    .call(DOUBLE, "js_perf_clear_marks", &[(DOUBLE, &a0)])
            }
            "clearMeasures" => {
                let a0 = lower_or_undef(ctx, 0)?;
                ctx.block()
                    .call(DOUBLE, "js_perf_clear_measures", &[(DOUBLE, &a0)])
            }
            "eventLoopUtilization" => {
                let a0 = lower_or_undef(ctx, 0)?;
                let a1 = lower_or_undef(ctx, 1)?;
                ctx.block().call(
                    DOUBLE,
                    "js_perf_event_loop_utilization",
                    &[(DOUBLE, &a0), (DOUBLE, &a1)],
                )
            }
            "toJSON" => ctx.block().call(DOUBLE, "js_perf_to_json", &[]),
            "clearResourceTimings" => {
                ctx.block()
                    .call(DOUBLE, "js_perf_clear_resource_timings", &[])
            }
            "setResourceTimingBufferSize" => {
                let a0 = lower_or_undef(ctx, 0)?;
                ctx.block().call(
                    DOUBLE,
                    "js_perf_set_resource_timing_buffer_size",
                    &[(DOUBLE, &a0)],
                )
            }
            // #1478: performance.markResourceTiming(info) appends a
            // PerformanceResourceTiming entry built from a
            // PerformanceTimingInfo-shaped object. Perry doesn't track
            // the resource-timing buffer yet — return undefined as a
            // no-op so feature-detect-and-call (`typeof X === "function"`
            // + invocation) doesn't crash. The accompanying read of
            // performance.getEntriesByType("resource") still returns
            // an empty array.
            "markResourceTiming" => undef.clone(),
            // #1335: performance.timerify(fn) wraps `fn` so each call
            // records a 'function' timeline entry; the wrapper still
            // returns fn's result. Perry doesn't track function
            // timings, so the simplest spec-compatible behavior is to
            // return `fn` itself — `typeof timerified === "function"`
            // still holds, calling it produces the original result,
            // and the only missing side effect is pushing an entry
            // into the PerformanceObserver "function" type (which
            // isn't in our supported entryTypes set today, so no
            // observer would see it anyway).
            "timerify" => lower_or_undef(ctx, 0)?,
            // #1336: perf_hooks.monitorEventLoopDelay(options?) returns
            // an IntervalHistogram; createHistogram(options?) returns a
            // RecordableHistogram. Both are stubs (every stat reads 0
            // and enable/disable/reset/record are no-ops), but the
            // returned object has the right shape for feature-detection
            // and trivial-call paths.
            "monitorEventLoopDelay" => {
                let a0 = lower_or_undef(ctx, 0)?;
                ctx.block()
                    .call(DOUBLE, "js_perf_monitor_event_loop_delay", &[(DOUBLE, &a0)])
            }
            "createHistogram" => {
                let a0 = lower_or_undef(ctx, 0)?;
                ctx.block()
                    .call(DOUBLE, "js_perf_create_histogram", &[(DOUBLE, &a0)])
            }
            _ => return Ok(None),
        };
        return Ok(Some(v));
    }

    Ok(None)
}
