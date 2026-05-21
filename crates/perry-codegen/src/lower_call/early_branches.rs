//! Early `lower_call` branches that fire before the big FuncRef /
//! ExternFuncRef / PropertyGet families:
//!
//! 1. `app.server.on(...)` and similar
//!    `nativeMethodCallReceiver.<prop>(args)` chains (#1113).
//! 2. `obj[strKey](args)` computed-key method call (v0.5.754).
//! 3. `CurrentStepClosure(args)` — async-step TLS dispatch (#691 P2).
//! 4. Closure-typed local call (`counter()` where `counter: () => void`).
//!
//! Each `try_lower_*` returns `Ok(Some(s))` when it handled the call,
//! `Ok(None)` to let the caller try the next branch.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::types::{DOUBLE, I64};

pub fn try_lower_native_chain_method_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // #1113 — `app.server.on(event, cb)` and similar
    // `nativeMethodCallReceiver.<prop>(args)` chains. The HIR shape
    // is `Call { callee: PropertyGet { object: NativeMethodCall {
    // module, … }, property: P }, args }` — `app.server` lowered as
    // `NativeMethodCall(module="fastify", method="server")` returning
    // the FastifyApp handle, but `.on(…)` then went through the
    // generic property-get path (because TypeScript's structural
    // typing on the return shape doesn't propagate the native-module
    // tag through `.server`). The property read returned undefined
    // and the call silently no-op'd (`(undefined)(…)` returns NaN in
    // Perry's runtime today — no exception). User code patterns like
    //
    //   app.server.on("upgrade", (req, socket, head) => …)
    //
    // therefore ran without throwing but never registered the
    // callback. Forward the call into the NATIVE_MODULE_TABLE arm
    // for `(module, P)` whenever the inner NativeMethodCall's module
    // recognises `P` as one of its methods (the dispatch table is
    // already the authoritative source for "what method names this
    // native module exposes"). Scoped narrowly — falls back to the
    // existing call lowering if the lookup misses.
    if let Expr::PropertyGet { object, property } = callee {
        if let Expr::NativeMethodCall { module, .. } = object.as_ref() {
            if super::native_module_lookup(module, true, property, None).is_some() {
                return Ok(Some(super::lower_native_method_call(
                    ctx,
                    module,
                    None,
                    property,
                    Some(object.as_ref()),
                    args,
                )?));
            }
        }
    }
    Ok(None)
}

pub fn try_lower_index_get_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // v0.5.754: `obj[strKey](args)` computed-key method call. Drizzle's
    // `this.session[isOneTimeQuery ? "prepareOneTimeQuery" : "prepareQuery"](...)`
    // lowers as Call { callee: IndexGet { object, index }, args }. Pre-fix
    // this fell through to the generic call path that read obj[index] as
    // a value (returning undefined for class methods) and then tried to
    // call undefined. Route through `js_native_call_method_str_key` which
    // walks the class vtable chain (parent inheritance included). Refs
    // #420 / #618 followup.
    if let Expr::IndexGet { object, index } = callee {
        if matches!(index.as_ref(), Expr::String(_))
            || crate::type_analysis::is_string_expr(ctx, index)
            || crate::type_analysis::is_definitely_string_expr(ctx, index)
        {
            let recv_box = lower_expr(ctx, object)?;
            let name_box = lower_expr(ctx, index)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            let n = lowered_args.len();
            let name_handle = {
                let blk = ctx.block();
                crate::expr::unbox_str_handle(blk, &name_box)
            };
            let (args_ptr, args_len) = if n == 0 {
                ("null".to_string(), "0".to_string())
            } else {
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf_reg
                ));
                (ptr_reg, n.to_string())
            };
            return Ok(Some(ctx.block().call(
                DOUBLE,
                "js_native_call_method_str_key",
                &[
                    (DOUBLE, &recv_box),
                    (I64, &name_handle),
                    (crate::types::PTR, &args_ptr),
                    (I64, &args_len),
                ],
            )));
        }
    }
    Ok(None)
}

pub fn try_lower_current_step_closure_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // #691 Phase 2: calling the current step closure via TLS.
    // `build_async_step_driver_direct` emits this for the catch arm's
    // `__step(e, true)` recursive re-entry — there's no captured
    // local to refer to anymore, so the callee is read out of TLS.
    // Dispatches through the same `js_closure_call<N>` family.
    if matches!(callee, Expr::CurrentStepClosure) {
        let recv_box = lower_expr(ctx, callee)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        if lowered_args.len() > 16 {
            bail!(
                "perry-codegen Phase D.1: CurrentStepClosure call with {} args (max 16)",
                lowered_args.len()
            );
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &recv_box);
        let runtime_fn = format!("js_closure_call{}", lowered_args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(Some(blk.call(DOUBLE, &runtime_fn, &call_args)));
    }
    Ok(None)
}

pub fn try_lower_closure_typed_local_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Closure-typed local call: `counter()` where `counter` is a
    // local of `Type::Function(...)`. Dispatch through the runtime
    // `js_closure_call<N>` family — the runtime extracts the function
    // pointer from the closure header and invokes it with the closure
    // as the first arg followed by the user args.
    if let Expr::LocalGet(id) = callee {
        if matches!(ctx.local_types.get(id), Some(HirType::Function(_))) {
            let recv_box = lower_expr(ctx, callee)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Issue #493: rest-bundling is now handled inside js_closure_callN
            // via the runtime closure-rest registry — see
            // `js_register_closure_rest` (registered for every closure body
            // with `...rest` at module init) and `dispatch_rest_bundled` in
            // `crates/perry-runtime/src/closure.rs`. Bundling at the static
            // call site here would double-wrap (the runtime would re-bundle
            // the already-bundled array into `[[a,b,c]]`), so the call site
            // now passes the raw args through and lets the runtime
            // pack the trailing tail into the rest slot.
            //
            // FuncRef calls (direct function-symbol dispatch) keep their
            // static-bundling at lower_call.rs:444+ because they don't go
            // through js_closure_callN.
            if lowered_args.len() > 16 {
                bail!(
                    "perry-codegen Phase D.1: closure call with {} args (max 16)",
                    lowered_args.len()
                );
            }
            let blk = ctx.block();
            let closure_handle = unbox_to_i64(blk, &recv_box);
            let runtime_fn = format!("js_closure_call{}", lowered_args.len());
            let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
            for v in &lowered_args {
                call_args.push((DOUBLE, v.as_str()));
            }
            return Ok(Some(blk.call(DOUBLE, &runtime_fn, &call_args)));
        }
    }
    Ok(None)
}
