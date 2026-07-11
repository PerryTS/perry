//! Native-method-call dispatcher: `lower_native_method_call`.
//!
//! Tier 2.2 follow-up (v0.5.340). The 805-LOC dispatcher routes
//! `obj.method(args)` calls against native modules (mysql2, pg, redis,
//! mongo, ws, fastify, fetch, perry/ui, perry/system, perry/i18n,
//! perry/plugin, AbortController, …) to their runtime FFI symbols. It
//! also handles a handful of receiver-less perry/ui forms (`Text(...)`,
//! `Button(...)`) that previously routed here before the v0.5.10
//! perry-ui table extraction.
//!
//! 14 helper cross-references reach back into the parent module via
//! `super::` (perry_*_table_lookup family, native_module_lookup,
//! lower_perry_ui_table_call, lower_fetch_native_method,
//! lower_abort_controller_call, lower_notification_schedule, …).
//! All were bumped from private `fn` to `pub(super) fn` in this PR.
//!
//! Split into siblings:
//! - `box_style.rs` — `apply_box_style` + `emit_dim_setter` (perry/tui
//!   `Box(...)` inline-style destructure helpers).
//! - `jsonwebtoken.rs` — `lower_jsonwebtoken_sign` / `_verify` (#1074
//!   algorithm-aware routing).
//! The giant `lower_native_method_call` dispatcher itself stays here.

use anyhow::{bail, Result};
use perry_dispatch::{ArgKind as UiArgKind, ReturnKind as UiReturnKind};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{
    emit_root_nanbox_store_on_block, lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx,
};
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::types::{DOUBLE, I32, I64, PTR};

// Re-export parent items so siblings can pick them up via `use super::*;`.
// `pub(super)` is the visibility-tightest form that still lets the glob
// fire from children — `use super::name` (no `pub`) keeps the item
// scoped to mod.rs only and `super::*` from siblings can't see it.
pub(super) use super::{
    apply_inline_style, collect_closure_introduced_ids, extract_options_fields,
    find_outer_writes_stmt, find_thread_hazard_in_body, get_raw_string_ptr,
    hazardous_module_global_ids, lower_fetch_native_method, lower_native_module_dispatch,
    lower_notification_schedule, lower_perry_ui_table_call, native_module_lookup,
    perry_audio_table_lookup, perry_i18n_table_lookup, perry_media_table_lookup,
    perry_plugin_instance_method_lookup, perry_plugin_table_lookup, perry_system_table_lookup,
    perry_ui_instance_method_lookup, perry_ui_table_lookup, perry_updater_table_lookup,
    ThreadClosureHazard,
};

mod box_style;
mod jsonwebtoken;
mod perf_hooks;

use box_style::apply_box_style;
use jsonwebtoken::{lower_jsonwebtoken_sign, lower_jsonwebtoken_verify};

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

pub(crate) fn lower_native_method_call(
    ctx: &mut FnCtx<'_>,
    module: &str,
    class_name: Option<&str>,
    method: &str,
    object: Option<&Expr>,
    args: &[Expr],
) -> Result<String> {
    include!("native_runtime_branch.rs");
    include!("native_tui_layout_branch.rs");
    include!("native_ui_widgets_branch.rs");
    include!("native_ui_appshell_branch.rs");
    include!("native_fs_branch.rs");

    // process module functions: cwd / uptime / memoryUsage / versions
    // accessed as destructured imports. `import { cwd } from 'node:process'`
    // → NativeMethodCall { module: "process", method: "cwd", object: None }.
    // The implicit-global form `process.cwd()` is already lowered to
    // dedicated HIR variants (Expr::ProcessCwd etc) in
    // perry-hir/src/lower/expr_call.rs:262, so the runtime helpers
    // (js_process_cwd / js_process_uptime / js_process_versions /
    // js_process_memory_usage) already exist — this arm just routes the
    // destructured-import shape to the same helpers. Closes #360 item #2's
    // dispatch gap (the warning fix alone would link cwd() but return
    // undefined silently — worse UX than the original "Could not resolve").
    if module == "process" && object.is_none() {
        match method {
            "cwd" => {
                let blk = ctx.block();
                let h = blk.call(I64, "js_process_cwd", &[]);
                return Ok(crate::expr::nanbox_string_inline(blk, &h));
            }
            "uptime" => {
                return Ok(ctx.block().call(DOUBLE, "js_process_uptime", &[]));
            }
            "memoryUsage" => {
                return Ok(ctx.block().call(DOUBLE, "js_process_memory_usage", &[]));
            }
            _ => {
                // Unknown process method — fall through to the generic
                // dispatch which will emit a diagnostic if no signature
                // matches. Likely candidates not wired here: nextTick
                // (needs a callback arg), exit (takes a code), kill,
                // hrtime. Each is its own follow-up under #360.
            }
        }
    }

    // Generic native module dispatch (receiver-less): fastify, mysql2,
    // ws, pg, ioredis, mongodb, better-sqlite3, etc. These were in the
    // old Cranelift codegen's dispatch table but lost in the v0.5.0
    // LLVM cutover.
    if object.is_none() {
        if let Some(sig) = native_module_lookup(module, false, method, class_name) {
            // perry/thread thread-safety check: the closure passed to
            // parallelMap / parallelFilter / spawn must not write to any
            // variable declared outside its own body. Each worker thread
            // gets its own deep-copied snapshot of ordinary captures, and
            // module-level variables live in global slots that would race
            // across workers — either way, writes are silently lost or
            // corrupted relative to user expectations. Enforce at compile
            // time so the docs' promise is real.
            //
            // Note we can't rely on the closure's `mutable_captures` field
            // alone: the HIR filters module-level IDs out of `captures`
            // via `filter_module_level_captures` (see lower.rs:457), so a
            // top-level `let counter = 0; parallelMap(data, () => counter++)`
            // ends up with `captures: [], mutable_captures: []` even though
            // the body obviously writes to `counter`. Instead, walk the
            // body ourselves and flag any LocalSet/Update whose target
            // isn't a parameter or a `let` introduced inside the body.
            if module == "perry/thread" {
                let closure_arg = match method {
                    "parallelMap" | "parallelFilter" => args.get(1),
                    "spawn" => args.first(),
                    _ => None,
                };
                if let Some(callback) = closure_arg {
                    match callback {
                        Expr::Closure {
                            func_id,
                            is_async,
                            params,
                            body,
                            ..
                        } => {
                            let mut inner_ids: std::collections::HashSet<perry_types::LocalId> =
                                params.iter().map(|p| p.id).collect();
                            for stmt in body {
                                collect_closure_introduced_ids(stmt, &mut inner_ids);
                            }
                            let mut outer_writes: Vec<perry_types::LocalId> = Vec::new();
                            for stmt in body {
                                find_outer_writes_stmt(stmt, &inner_ids, &mut outer_writes);
                            }
                            if let Some(&first_outer) = outer_writes.first() {
                                anyhow::bail!(
                                    "perry/thread: closure passed to `{}` writes to outer variable (LocalId {}) — \
                                     this is not allowed because each worker thread receives a deep-copied \
                                     snapshot of captured values (and module-level slots are not shared across \
                                     workers in the way ordinary TS globals appear to be), so writes would be \
                                     silently lost or corrupted relative to user expectations. Return values \
                                     from the closure and aggregate them on the main thread instead. \
                                     See docs/src/threading/overview.md#no-shared-mutable-state.",
                                    method, first_outer,
                                );
                            }
                            // #6185 Tier-1 containment: reject async work,
                            // nested thread primitives, and reads of
                            // heap-typed module globals inside the worker
                            // body. Each is a cross-heap unsoundness: the
                            // await loop drains process-global queues on
                            // whatever thread runs it, and module globals
                            // are process-wide slots read in place
                            // (bypassing the capture deep-copy).
                            let worker_is_async =
                                *is_async || ctx.async_step_closures.contains(func_id);
                            let hazard = if worker_is_async {
                                Some(ThreadClosureHazard::AsyncClosure)
                            } else {
                                let hazardous_ids = hazardous_module_global_ids(
                                    ctx.module_globals,
                                    &ctx.local_types,
                                );
                                find_thread_hazard_in_body(
                                    body,
                                    &hazardous_ids,
                                    ctx.async_step_closures,
                                )
                            };
                            match hazard {
                                None => {}
                                Some(
                                    ThreadClosureHazard::AsyncClosure | ThreadClosureHazard::Await,
                                ) => {
                                    anyhow::bail!(
                                        "perry/thread: closure passed to `{}` must be synchronous — it is \
                                         (or contains) an async closure or `await`. An `await` inside a \
                                         worker runs the process-wide completion/timer pump on the worker \
                                         thread: it can steal another thread's completion, fire foreign-heap \
                                         timer callbacks, and resolve main-heap promises with pointers into \
                                         the worker's arena, which is unmapped when the worker exits \
                                         (use-after-free; issue #6185). Do the async work on the main thread \
                                         and pass plain data into the worker, or return a value from the \
                                         worker and `await` the {} result on the main thread instead.",
                                        method, method,
                                    );
                                }
                                Some(ThreadClosureHazard::NestedThreadCall(inner)) => {
                                    anyhow::bail!(
                                        "perry/thread: `{}` may not be called inside a closure passed to \
                                         `{}` — nested thread primitives make the worker pump the \
                                         process-global thread-completion queue, which can deserialize \
                                         another thread's result into the wrong arena (issue #6185). \
                                         Restructure so all `spawn`/`parallelMap`/`parallelFilter` calls \
                                         happen on the main thread.",
                                        inner, method,
                                    );
                                }
                                Some(ThreadClosureHazard::ModuleGlobalAccess(id)) => {
                                    anyhow::bail!(
                                        "perry/thread: closure passed to `{}` accesses a module-scope \
                                         variable (LocalId {}) whose value is a heap object (object, array, \
                                         function, Map/Set, class instance, ...). Module-level bindings are \
                                         process-wide slots read in place — they do NOT go through the \
                                         capture deep-copy — so the worker thread would alias objects on the \
                                         main thread's heap with no synchronization (issue #6185). Bind the \
                                         value to a function-scope local first (`const copy = theGlobal;`) so \
                                         the closure captures it and it is deep-copied to the worker, or \
                                         declare module-level helpers with `function name(...)` instead of \
                                         `const name = (...) => ...`. Primitive module globals (numbers, \
                                         strings, booleans) are unaffected. \
                                         See docs/src/threading/overview.md#no-shared-mutable-state.",
                                        method, id,
                                    );
                                }
                            }
                        }
                        // Named-function callback bypass: `function worker(n) { counter++; }
                        // parallelMap(xs, worker)` is semantically identical to the inline-
                        // closure form we check above, but we don't have the callee's HIR
                        // body accessible from FnCtx (only `func_names: FuncId -> String`,
                        // not the full function table). Bail with a helpful diagnostic
                        // pointing the user at the inline-closure workaround. Pure
                        // function workers work fine when wrapped (`(x) => worker(x)`);
                        // this just closes the compile-time safety bypass that silently
                        // let outer-writing named functions through.
                        Expr::FuncRef(_) | Expr::LocalGet(_) | Expr::ExternFuncRef { .. } => {
                            anyhow::bail!(
                                "perry/thread: `{}` callback must be an inline arrow/closure, not a \
                                 named function reference. Compile-time thread-safety analysis can only \
                                 inspect inline closures today; a named function could write to outer \
                                 variables which would be silently lost on the deep-copy worker boundary. \
                                 Workaround: wrap the named function in an inline closure — \
                                 `{}(xs, (x) => myFn(x))`. See docs/src/threading/overview.md#no-shared-mutable-state.",
                                method, method,
                            );
                        }
                        _ => {}
                    }
                }
            }
            return lower_native_module_dispatch(ctx, sig, None, args);
        }
    }

    // #1002: native `util.format` / `util.formatWithOptions`. The HIR
    // surfaces these as receiver-less `NativeMethodCall { module:
    // "util", method: "format" | "formatWithOptions", args }`. Before
    // this arm, both fell into the receiver-less fall-through below
    // and returned `TAG_UNDEFINED` — `console.log(util.format("hi %s",
    // "x"))` printed `undefined` and `test_util_format_with_options`
    // failed the parity gate on every PR.
    //
    // Bundle the substitution args into a heap array (same shape
    // `js_console_log_spread` uses). For `formatWithOptions(opts, fmt,
    // ...args)`, pass arg[0] separately so the runtime can apply the
    // inspect options around `%O` / `%o`.
    if module == "util" && object.is_none() && (method == "format" || method == "formatWithOptions")
    {
        let options_value = if method == "formatWithOptions" {
            if let Some(options_arg) = args.first() {
                Some(lower_expr(ctx, options_arg)?)
            } else {
                Some(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
            }
        } else {
            None
        };
        let skip = usize::from(method == "formatWithOptions");
        let payload: Vec<&Expr> = args.iter().skip(skip).collect();
        let cap = (payload.len() as u32).to_string();
        let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for arg in &payload {
            let v = lower_expr(ctx, arg)?;
            let blk = ctx.block();
            current_arr = blk.call(
                I64,
                "js_array_push_f64",
                &[(I64, &current_arr), (DOUBLE, &v)],
            );
        }
        let blk = ctx.block();
        let result = if let Some(options_value) = options_value {
            blk.call(
                DOUBLE,
                "js_util_format_with_options",
                &[(DOUBLE, &options_value), (I64, &current_arr)],
            )
        } else {
            blk.call(DOUBLE, "js_util_format", &[(I64, &current_arr)])
        };
        return Ok(result);
    }

    // `new Console({ stdout, stderr }).log(...)` reaches HIR as an instance
    // `NativeMethodCall` on module "console" / class "Console", not as a
    // generic `PropertyGet` call. Preserve the receiver and route through the
    // runtime's method dispatcher so per-instance streams, indentation, and
    // counters are used instead of the process-global console helpers.
    if module == "console" && class_name == Some("Console") {
        if let Some(recv) = object {
            let recv_box = lower_expr(ctx, recv)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }

            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                {
                    let blk = ctx.block();
                    for (i, value) in lowered_args.iter().enumerate() {
                        let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                        blk.store(DOUBLE, value, &slot);
                    }
                }
                (buf, n.to_string())
            };

            let method_idx = ctx.strings.intern(method);
            let entry = ctx.strings.entry(method_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len = entry.byte_len.to_string();
            return Ok(ctx.block().call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len),
                    (PTR, &args_ptr),
                    (I64, &args_len),
                ],
            ));
        }
    }

    // Receiver-less native method calls (e.g. plugin::setConfig(...)
    // as a static module function): lower args for side effects and
    // return TAG_UNDEFINED. Using TAG_UNDEFINED (not 0.0) so that
    // downstream .length reads return 0 instead of crashing (the
    // inline .length guard checks ptr < 4096, and TAG_UNDEFINED's
    // lower 48 bits = 1).
    let Some(recv) = object else {
        // Named/value-form imports of node-core native-module functions
        // (`import { realpathSync } from "fs"; realpathSync(p)`) reach here
        // as a receiver-less `NativeMethodCall` with no static-table row.
        // Pre-fix they fell straight to the TAG_UNDEFINED sentinel below, so
        // the call returned `undefined` even though the member form
        // (`fs.realpathSync(p)`) works — the member form routes through the
        // runtime by-name dispatcher (`dispatch_native_module_method`), which
        // DOES implement these. Bridge the value form onto that same path by
        // synthesizing the module namespace receiver (exactly what
        // `NativeModuleRef(module)` lowers to) and dispatching the method on
        // it via `js_native_call_method`. Scope strictly to modules that own a
        // runtime dispatch bucket (`nm_install_symbol(module).is_some()`) so
        // perry/* internal modules and genuinely-unimplemented modules keep
        // the historical undefined fall-through and never mis-dispatch.
        // Fixes the realpathSync class (fs/os/path/url/... named-import fns).
        if crate::nm_install::nm_install_symbol(module).is_some() {
            let recv_box =
                crate::expr::lower_expr(ctx, &Expr::NativeModuleRef(module.to_string()))?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for arg in args {
                lowered_args.push(lower_expr(ctx, arg)?);
            }
            let (args_ptr, args_len) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                {
                    let blk = ctx.block();
                    for (i, value) in lowered_args.iter().enumerate() {
                        let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                        blk.store(DOUBLE, value, &slot);
                    }
                }
                (buf, n.to_string())
            };
            let method_idx = ctx.strings.intern(method);
            let entry = ctx.strings.entry(method_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len = entry.byte_len.to_string();
            return Ok(ctx.block().call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len),
                    (PTR, &args_ptr),
                    (I64, &args_len),
                ],
            ));
        }
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
    };
    let _ = (module, method); // shut up unused warnings on the early-out path

    include!("native_instance_branch.rs")
}
