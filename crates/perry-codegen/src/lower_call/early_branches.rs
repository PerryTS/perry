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

use crate::expr::{
    emit_typed_feedback_register_site, i32_bool_to_nanbox, lower_expr, nanbox_pointer_inline,
    unbox_to_i64, FnCtx, TypedFeedbackContract, TypedFeedbackKind,
};
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I32, I64};

fn typed_i1_closure_signature_note(reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature=i1(i64 closure, {first})->i1")
    } else {
        format!("typed_signature=i1(i64 closure, {first}, ...)->i1")
    }
}

fn typed_string_closure_signature_note(arg_count: usize) -> String {
    if arg_count <= 1 {
        "typed_signature=string(i64 closure, string)->string".to_string()
    } else {
        "typed_signature=string(i64 closure, string, ...)->string".to_string()
    }
}

fn typed_closure_signature_note(ret: &str, reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature={ret}(i64 closure, {first})->{ret}")
    } else {
        format!("typed_signature={ret}(i64 closure, {first}, ...)->{ret}")
    }
}

fn typed_i32_closure_signature_note(arg_count: usize) -> String {
    if arg_count <= 1 {
        "typed_signature=i32(i64 closure, i32)->i32".to_string()
    } else {
        "typed_signature=i32(i64 closure, i32, ...)->i32".to_string()
    }
}

fn is_async_dispose_symbol_index(index: &Expr) -> bool {
    let Expr::SymbolFor(symbol_name) = index else {
        return false;
    };
    match symbol_name.as_ref() {
        Expr::String(name) => name == "@@__perry_wk_asyncDispose",
        Expr::WtfString(name) => name.as_slice() == b"@@__perry_wk_asyncDispose",
        _ => false,
    }
}

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
    if let Expr::PropertyGet {
        object, property, ..
    } = callee
    {
        if let Some(module) = native_receiver_module(object.as_ref()) {
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

/// Resolve the native module a (possibly chained) receiver expression
/// evaluates to, for native-method chain dispatch (#1113 + fluent chains).
/// Returns the module name borrowed from `expr`.
///
/// - A `NativeMethodCall { module }` is directly that module's value (the
///   call-site `native_module_lookup` then decides whether the outer
///   `property` is a real method of it).
/// - A nested `Call { PropertyGet { object, property } }` is itself a chained
///   method call: it evaluates to `object`'s module iff that module's
///   `property` returns another instance of the same module (a fluent
///   transform). This recursion is what lets
///   `sharp(input).resize(w,h).jpeg().toBuffer()` keep dispatching natively
///   past the first link instead of falling to the generic
///   "(number) is not a function" runtime error. cheerio doesn't need it —
///   its chains are already rewritten to nested `NativeMethodCall`s by the
///   HIR `fix_local_native_instances` pass, so its receiver is matched by the
///   `NativeMethodCall` arm directly.
fn native_receiver_module(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::NativeMethodCall { module, .. } => Some(module.as_str()),
        Expr::Call { callee, .. } => {
            if let Expr::PropertyGet {
                object, property, ..
            } = callee.as_ref()
            {
                let module = native_receiver_module(object.as_ref())?;
                if native_method_returns_self_instance(module, property) {
                    return Some(module);
                }
            }
            None
        }
        _ => None,
    }
}

/// `(module, method)` pairs whose return value is another instance of the
/// SAME native module — fluent/builder methods that can be chained. Used by
/// [`native_receiver_module`] to thread a module identity through a chained
/// call. Terminals are intentionally excluded so a value of a different type
/// can't masquerade as a same-module instance (sharp `toBuffer`/`toFile`/
/// `metadata` → Promise, `width`/`height` → number).
fn native_method_returns_self_instance(module: &str, method: &str) -> bool {
    match module {
        // sharp image transforms, plus the factory call itself
        // (`sharp(input)` lowers to method "default"/"sharp"). Each returns
        // the Sharp instance for further chaining.
        //
        // This list mirrors the *dispatchable* fluent methods — every name
        // here also has an instance-returning (`NR_PTR`) row in
        // `native_table/media.rs`. Add a name here only together with its
        // `media.rs` row, otherwise `recv.<name>()` wouldn't resolve and the
        // chain's terminal would run on a garbage receiver.
        "sharp" => matches!(
            method,
            "default"
                | "sharp"
                | "resize"
                | "rotate"
                | "flip"
                | "flop"
                | "grayscale"
                | "blur"
                | "sharpen"
                | "extract"
                | "autoOrient"
                | "extend"
                | "trim"
                | "composite"
                | "jpeg"
                | "png"
                | "webp"
                | "avif"
        ),
        _ => false,
    }
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
        // Don't intercept array/typed-array element calls keyed by a numeric
        // expression — those have dedicated lowering and aren't method
        // dispatch. Class refs are the exception: `C[1]()` is a static
        // computed method call after ToPropertyKey canonicalizes `1` to "1".
        //
        // The receiver must actually be an array for that bail to be sound. A
        // numeric key alone is not enough: `obj[k]()` on a *plain object* is
        // still a method call and must bind `this = obj` (#6328). The element
        // lowering reads the slot and calls it as a bare closure, dropping the
        // receiver. That used to be unreachable here because in an `async` body
        // the async-to-generator transform boxes body locals, so `k`'s numeric
        // type was invisible and this guard never fired; #6369 restores those
        // declared types, which made the plain-object case reachable and
        // resurrected the `this === undefined` bug. Gate on the receiver being
        // a real array so non-arrays fall through to the dispatch tower below,
        // which binds `this` for both numeric and string keys.
        let object_is_class_ref = matches!(object.as_ref(), Expr::ClassRef(_))
            || matches!(object.as_ref(), Expr::ExternFuncRef { name, .. } if ctx.class_ids.contains_key(name));
        if crate::type_analysis::is_numeric_expr(ctx, index)
            && crate::type_analysis::is_array_expr(ctx, object)
            && !object_is_class_ref
        {
            return Ok(None);
        }
        if crate::type_analysis::receiver_class_name(ctx, object).as_deref() == Some("Server")
            && is_async_dispose_symbol_index(index)
        {
            let recv_box = lower_expr(ctx, object)?;
            for arg in args {
                let _ = lower_expr(ctx, arg)?;
            }
            let blk = ctx.block();
            let handle = unbox_to_i64(blk, &recv_box);
            blk.call_void("js_net_server_close", &[(I64, &handle), (I64, "0")]);
            let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let promise_handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &undef)]);
            return Ok(Some(nanbox_pointer_inline(blk, &promise_handle)));
        }
        let is_static_string = matches!(index.as_ref(), Expr::String(_))
            || crate::type_analysis::is_string_expr(ctx, index)
            || crate::type_analysis::is_definitely_string_expr(ctx, index);

        let recv_box = lower_expr(ctx, object)?;
        let key_box = lower_expr(ctx, index)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        let n = lowered_args.len();
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

        if is_static_string {
            // Statically-known string key: extract the string handle and use
            // the str-key entry (`this` bound by the dispatch tower).
            let name_handle = {
                let blk = ctx.block();
                crate::expr::unbox_str_handle(blk, &key_box)
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

        // Dynamic key (`this[(cur)._op](cur)`, `obj[k]()` where `k` is a
        // runtime value): pass the key value through, the runtime branches on
        // its type and binds `this = obj` either way. Refs #321 (effect
        // FiberRuntime op dispatch) — pre-fix this fell through to a plain
        // closure-call that dropped `this`, so a method stored as a class
        // field reached by dynamic key read `this === undefined`.
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_native_call_method_value",
            &[
                (DOUBLE, &recv_box),
                (DOUBLE, &key_box),
                (crate::types::PTR, &args_ptr),
                (I64, &args_len),
            ],
        )));
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
            let closure_handle = {
                let blk = ctx.block();
                unbox_to_i64(blk, &recv_box)
            };
            // Receiverless call of a closure-typed local: bind `this` to
            // undefined for the duration of the call (OrdinaryCallBindThis,
            // #3576) so an enclosing method dispatch's IMPLICIT_THIS does
            // not leak into the callee body. Like the FuncRef path, the
            // reset is gated on the statically-known callee actually reading
            // dynamic `this`, so a hot-loop call of a plain helper closure
            // pays nothing (#5030). When the typed-feedback guard falls back
            // (the receiver is NOT the statically-mapped closure), the
            // fallback block does its own reset — that callee is unknown.
            let undef_this =
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let known_func_id = ctx.local_closure_func_ids.get(id).copied();
            let callee_reads_this = known_func_id
                .map(|fid| ctx.funcs_reading_dynamic_this.contains(&fid))
                .unwrap_or(true);
            if let Some(func_id) = known_func_id {
                let declared_count = ctx
                    .local_closure_param_counts
                    .get(id)
                    .copied()
                    .unwrap_or(lowered_args.len());
                let has_rest = ctx.closure_rest_params.contains_key(&func_id);
                if !has_rest && declared_count == lowered_args.len() {
                    let closure_fn =
                        format!("perry_closure_{}__{}", ctx.strings.module_prefix(), func_id);
                    let site_id = emit_typed_feedback_register_site(
                        ctx,
                        TypedFeedbackKind::ClosureCall,
                        &format!("closure:{}", func_id),
                        TypedFeedbackContract::closure_direct_call(),
                    );
                    let prev_this = if callee_reads_this {
                        Some(ctx.block().call(
                            DOUBLE,
                            "js_implicit_this_set",
                            &[(DOUBLE, &undef_this)],
                        ))
                    } else {
                        None
                    };
                    let expected_arity = declared_count.to_string();
                    let call_arity = lowered_args.len().to_string();
                    let guard_ok = ctx.block().call(
                        I32,
                        "js_typed_feedback_closure_direct_call_guard",
                        &[
                            (I64, &site_id),
                            (DOUBLE, &recv_box),
                            (crate::types::PTR, &format!("@{}", closure_fn)),
                            (I32, &expected_arity),
                            (I32, &call_arity),
                        ],
                    );
                    let guard_pass = ctx.block().icmp_ne(I32, &guard_ok, "0");
                    let fast_idx = ctx.new_block("closure_direct.fast");
                    let fallback_idx = ctx.new_block("closure_direct.fallback");
                    let merge_idx = ctx.new_block("closure_direct.merge");
                    let fast_label = ctx.block_label(fast_idx);
                    let fallback_label = ctx.block_label(fallback_idx);
                    let merge_label = ctx.block_label(merge_idx);
                    ctx.block()
                        .cond_br(&guard_pass, &fast_label, &fallback_label);

                    ctx.current_block = fast_idx;
                    let typed_f64_param_reps = if ctx.typed_f64_closures.contains(&func_id) {
                        ctx.typed_i1_closure_param_reps
                            .get(&func_id)
                            .filter(|reps| {
                                crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                            })
                            .cloned()
                    } else {
                        None
                    };
                    let typed_i32_param_reps = if ctx.typed_i32_closures.contains(&func_id) {
                        ctx.typed_i1_closure_param_reps
                            .get(&func_id)
                            .filter(|reps| {
                                crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                            })
                            .cloned()
                    } else {
                        None
                    };
                    let typed_string_param_reps = if ctx.typed_string_closures.contains(&func_id) {
                        ctx.typed_i1_closure_param_reps
                            .get(&func_id)
                            .filter(|reps| {
                                crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                            })
                            .cloned()
                    } else {
                        None
                    };
                    let typed_i1_param_reps = if ctx.typed_i1_closures.contains(&func_id) {
                        if let Some(reps) = ctx.typed_i1_closure_param_reps.get(&func_id) {
                            let matches_args = reps.len() == args.len()
                                && args.iter().zip(reps.iter()).all(|(arg, rep)| match rep {
                                    crate::codegen::TypedParamRep::F64 => {
                                        crate::type_analysis::is_numeric_expr(ctx, arg)
                                    }
                                    crate::codegen::TypedParamRep::I32 => {
                                        matches!(
                                            crate::type_analysis::static_type_of(ctx, arg),
                                            Some(HirType::Int32)
                                        ) || matches!(
                                            arg,
                                            Expr::Integer(n)
                                                if (i64::from(i32::MIN)
                                                    ..=i64::from(i32::MAX))
                                                    .contains(n)
                                        )
                                    }
                                    crate::codegen::TypedParamRep::I1 => {
                                        crate::type_analysis::is_bool_expr(ctx, arg)
                                    }
                                    crate::codegen::TypedParamRep::StringRef => {
                                        crate::type_analysis::is_definitely_string_expr(ctx, arg)
                                    }
                                });
                            matches_args.then(|| reps.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let fast_value = if let Some(typed_param_reps) = typed_f64_param_reps {
                        let typed_fn = crate::codegen::typed_f64_closure_name(&closure_fn);
                        let generic_closure_fn =
                            crate::codegen::generic_closure_body_name(&closure_fn);
                        let mut numeric_guard: Option<String> = None;
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                            numeric_guard = Some(match numeric_guard {
                                Some(prev) => ctx.block().and(I1, &prev, &ok),
                                None => ok,
                            });
                        }

                        let typed_idx = ctx.new_block("closure_direct.typed_f64");
                        let generic_idx = ctx.new_block("closure_direct.generic");
                        let typed_merge_idx = ctx.new_block("closure_direct.typed_merge");
                        let typed_label = ctx.block_label(typed_idx);
                        let generic_label = ctx.block_label(generic_idx);
                        let typed_merge_label = ctx.block_label(typed_merge_idx);
                        if let Some(numeric_guard) = numeric_guard {
                            ctx.block()
                                .cond_br(&numeric_guard, &typed_label, &generic_label);
                        } else {
                            ctx.block().br(&typed_label);
                        }

                        ctx.current_block = typed_idx;
                        let mut typed_args_storage: Vec<String> =
                            Vec::with_capacity(lowered_args.len());
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                                ctx.block(),
                                *rep,
                                value,
                            ));
                        }
                        let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                            Vec::with_capacity(typed_args_storage.len() + 1);
                        typed_args.push((I64, &closure_handle));
                        typed_args.extend(
                            typed_args_storage
                                .iter()
                                .zip(typed_param_reps.iter())
                                .map(|(s, rep)| (rep.llvm_ty(), s.as_str())),
                        );
                        let typed_value = ctx.block().call(DOUBLE, &typed_fn, &typed_args);
                        let after_typed = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = generic_idx;
                        let mut generic_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, &closure_handle)];
                        for v in &lowered_args {
                            generic_args.push((DOUBLE, v.as_str()));
                        }
                        let generic_value =
                            ctx.block().call(DOUBLE, &generic_closure_fn, &generic_args);
                        let after_generic = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = typed_merge_idx;
                        let result = ctx.block().phi(
                            DOUBLE,
                            &[
                                (typed_value.as_str(), after_typed.as_str()),
                                (generic_value.as_str(), after_generic.as_str()),
                            ],
                        );
                        ctx.record_lowered_value(
                            "ClosureCall",
                            None,
                            "typed_f64_closure_direct_call",
                            &LoweredValue::f64(result.clone()),
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("typed_clone={typed_fn}"),
                                format!("generic_closure={generic_closure_fn}"),
                                format!("closure_func_id={func_id}"),
                                typed_closure_signature_note("f64", &typed_param_reps),
                            ],
                        );
                        result
                    } else if let Some(typed_param_reps) = typed_i32_param_reps {
                        let typed_fn = crate::codegen::typed_i32_closure_name(&closure_fn);
                        let generic_closure_fn =
                            crate::codegen::generic_closure_body_name(&closure_fn);
                        let mut typed_guard: Option<String> = None;
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                            typed_guard = Some(match typed_guard {
                                Some(prev) => ctx.block().and(I1, &prev, &ok),
                                None => ok,
                            });
                        }

                        let typed_idx = ctx.new_block("closure_direct.typed_i32");
                        let generic_idx = ctx.new_block("closure_direct.generic");
                        let typed_merge_idx = ctx.new_block("closure_direct.typed_merge");
                        let typed_label = ctx.block_label(typed_idx);
                        let generic_label = ctx.block_label(generic_idx);
                        let typed_merge_label = ctx.block_label(typed_merge_idx);
                        if let Some(typed_guard) = typed_guard {
                            ctx.block()
                                .cond_br(&typed_guard, &typed_label, &generic_label);
                        } else {
                            ctx.block().br(&typed_label);
                        }

                        ctx.current_block = typed_idx;
                        let mut typed_args_storage: Vec<String> =
                            Vec::with_capacity(lowered_args.len());
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                                ctx.block(),
                                *rep,
                                value,
                            ));
                        }
                        let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                            Vec::with_capacity(typed_args_storage.len() + 1);
                        typed_args.push((I64, &closure_handle));
                        typed_args.extend(
                            typed_args_storage
                                .iter()
                                .zip(typed_param_reps.iter())
                                .map(|(s, rep)| (rep.llvm_ty(), s.as_str())),
                        );
                        let raw_i32 = ctx.block().call(I32, &typed_fn, &typed_args);
                        let typed_value = crate::expr::i32_to_nanbox(ctx.block(), &raw_i32);
                        let after_typed = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = generic_idx;
                        let mut generic_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, &closure_handle)];
                        for v in &lowered_args {
                            generic_args.push((DOUBLE, v.as_str()));
                        }
                        let generic_value =
                            ctx.block().call(DOUBLE, &generic_closure_fn, &generic_args);
                        let after_generic = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = typed_merge_idx;
                        let result = ctx.block().phi(
                            DOUBLE,
                            &[
                                (typed_value.as_str(), after_typed.as_str()),
                                (generic_value.as_str(), after_generic.as_str()),
                            ],
                        );
                        ctx.record_lowered_value(
                            "ClosureCall",
                            None,
                            "typed_i32_closure_direct_call",
                            &LoweredValue::js_value(result.clone()),
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("typed_clone={typed_fn}"),
                                format!("generic_closure={generic_closure_fn}"),
                                format!("closure_func_id={func_id}"),
                                typed_closure_signature_note("i32", &typed_param_reps),
                                "boxed_result_at=direct_call_boundary".to_string(),
                            ],
                        );
                        result
                    } else if let Some(typed_param_reps) = typed_string_param_reps {
                        let typed_fn = crate::codegen::typed_string_closure_name(&closure_fn);
                        let generic_closure_fn =
                            crate::codegen::generic_closure_body_name(&closure_fn);
                        let mut typed_guard: Option<String> = None;
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
                            typed_guard = Some(match typed_guard {
                                Some(prev) => ctx.block().and(I1, &prev, &ok),
                                None => ok,
                            });
                        }
                        let capture_count = ctx
                            .typed_string_closure_capture_counts
                            .get(&func_id)
                            .copied()
                            .unwrap_or(0);
                        if capture_count > 0 {
                            if let Some(capture_guard) =
                                crate::codegen::emit_typed_string_capture_guard(
                                    ctx.block(),
                                    &closure_handle,
                                    capture_count,
                                )
                            {
                                typed_guard = Some(match typed_guard {
                                    Some(prev) => ctx.block().and(I1, &prev, &capture_guard),
                                    None => capture_guard,
                                });
                            }
                        }

                        let typed_idx = ctx.new_block("closure_direct.typed_string");
                        let generic_idx = ctx.new_block("closure_direct.generic");
                        let typed_merge_idx = ctx.new_block("closure_direct.typed_merge");
                        let typed_label = ctx.block_label(typed_idx);
                        let generic_label = ctx.block_label(generic_idx);
                        let typed_merge_label = ctx.block_label(typed_merge_idx);
                        if let Some(typed_guard) = typed_guard {
                            ctx.block()
                                .cond_br(&typed_guard, &typed_label, &generic_label);
                        } else {
                            ctx.block().br(&typed_label);
                        }

                        ctx.current_block = typed_idx;
                        let mut typed_args_storage: Vec<String> =
                            Vec::with_capacity(lowered_args.len());
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                                ctx.block(),
                                *rep,
                                value,
                            ));
                        }
                        let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                            Vec::with_capacity(typed_args_storage.len() + 1);
                        typed_args.push((I64, &closure_handle));
                        typed_args.extend(
                            typed_args_storage
                                .iter()
                                .zip(typed_param_reps.iter())
                                .map(|(s, rep)| (rep.llvm_ty(), s.as_str())),
                        );
                        let raw_string = ctx.block().call(I64, &typed_fn, &typed_args);
                        let typed_value =
                            ctx.block()
                                .call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)]);
                        let after_typed = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = generic_idx;
                        let mut generic_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, &closure_handle)];
                        for v in &lowered_args {
                            generic_args.push((DOUBLE, v.as_str()));
                        }
                        let generic_value =
                            ctx.block().call(DOUBLE, &generic_closure_fn, &generic_args);
                        let after_generic = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = typed_merge_idx;
                        let result = ctx.block().phi(
                            DOUBLE,
                            &[
                                (typed_value.as_str(), after_typed.as_str()),
                                (generic_value.as_str(), after_generic.as_str()),
                            ],
                        );
                        ctx.record_lowered_value(
                            "ClosureCall",
                            None,
                            "typed_string_closure_direct_call",
                            &LoweredValue::js_value(result.clone()),
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("typed_clone={typed_fn}"),
                                format!("generic_closure={generic_closure_fn}"),
                                format!("closure_func_id={func_id}"),
                                typed_closure_signature_note("string", &typed_param_reps),
                                "boxed_result_at=direct_call_boundary".to_string(),
                            ],
                        );
                        result
                    } else if let Some(typed_param_reps) = typed_i1_param_reps {
                        let typed_fn = crate::codegen::typed_i1_closure_name(&closure_fn);
                        let generic_closure_fn =
                            crate::codegen::generic_closure_body_name(&closure_fn);
                        let mut typed_guard: Option<String> = None;
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            let raw =
                                ctx.block()
                                    .call(I32, rep.guard_fn(), &[(DOUBLE, value.as_str())]);
                            let ok = ctx.block().icmp_ne(I32, &raw, "0");
                            typed_guard = Some(match typed_guard {
                                Some(prev) => ctx.block().and(I1, &prev, &ok),
                                None => ok,
                            });
                        }

                        let typed_idx = ctx.new_block("closure_direct.typed_i1");
                        let generic_idx = ctx.new_block("closure_direct.generic");
                        let typed_merge_idx = ctx.new_block("closure_direct.typed_merge");
                        let typed_label = ctx.block_label(typed_idx);
                        let generic_label = ctx.block_label(generic_idx);
                        let typed_merge_label = ctx.block_label(typed_merge_idx);
                        if let Some(typed_guard) = typed_guard {
                            ctx.block()
                                .cond_br(&typed_guard, &typed_label, &generic_label);
                        } else {
                            ctx.block().br(&typed_label);
                        }

                        ctx.current_block = typed_idx;
                        let mut typed_args_storage: Vec<String> =
                            Vec::with_capacity(lowered_args.len());
                        for (value, rep) in lowered_args.iter().zip(typed_param_reps.iter()) {
                            typed_args_storage.push(match rep {
                                crate::codegen::TypedParamRep::F64 => ctx.block().call(
                                    DOUBLE,
                                    rep.unbox_fn(),
                                    &[(DOUBLE, value.as_str())],
                                ),
                                crate::codegen::TypedParamRep::I32 => ctx.block().call(
                                    I32,
                                    rep.unbox_fn(),
                                    &[(DOUBLE, value.as_str())],
                                ),
                                crate::codegen::TypedParamRep::I1 => {
                                    let raw_i32 = ctx.block().call(
                                        I32,
                                        rep.unbox_fn(),
                                        &[(DOUBLE, value.as_str())],
                                    );
                                    ctx.block().icmp_ne(I32, &raw_i32, "0")
                                }
                                crate::codegen::TypedParamRep::StringRef => ctx.block().call(
                                    I64,
                                    rep.unbox_fn(),
                                    &[(DOUBLE, value.as_str())],
                                ),
                            });
                        }
                        let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                            Vec::with_capacity(typed_args_storage.len() + 1);
                        typed_args.push((I64, &closure_handle));
                        typed_args.extend(
                            typed_args_storage
                                .iter()
                                .zip(typed_param_reps.iter())
                                .map(|(s, rep)| (rep.llvm_ty(), s.as_str())),
                        );
                        let typed_i1 = ctx.block().call(I1, &typed_fn, &typed_args);
                        let typed_i32 = ctx.block().zext(I1, &typed_i1, I32);
                        let typed_value = i32_bool_to_nanbox(ctx.block(), &typed_i32);
                        let after_typed = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = generic_idx;
                        let mut generic_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, &closure_handle)];
                        for v in &lowered_args {
                            generic_args.push((DOUBLE, v.as_str()));
                        }
                        let generic_value =
                            ctx.block().call(DOUBLE, &generic_closure_fn, &generic_args);
                        let after_generic = ctx.block().label.clone();
                        if !ctx.block().is_terminated() {
                            ctx.block().br(&typed_merge_label);
                        }

                        ctx.current_block = typed_merge_idx;
                        let result = ctx.block().phi(
                            DOUBLE,
                            &[
                                (typed_value.as_str(), after_typed.as_str()),
                                (generic_value.as_str(), after_generic.as_str()),
                            ],
                        );
                        ctx.record_lowered_value(
                            "ClosureCall",
                            None,
                            "typed_i1_closure_direct_call",
                            &LoweredValue::js_value(result.clone()),
                            None,
                            None,
                            None,
                            false,
                            false,
                            vec![
                                format!("typed_clone={typed_fn}"),
                                format!("generic_closure={generic_closure_fn}"),
                                format!("closure_func_id={func_id}"),
                                typed_i1_closure_signature_note(&typed_param_reps),
                                "boxed_result_at=direct_call_boundary".to_string(),
                            ],
                        );
                        result
                    } else {
                        let mut direct_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, &closure_handle)];
                        for v in &lowered_args {
                            direct_args.push((DOUBLE, v.as_str()));
                        }
                        ctx.block().call(DOUBLE, &closure_fn, &direct_args)
                    };
                    let after_fast = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }

                    ctx.current_block = fallback_idx;
                    ctx.block()
                        .call_void("js_typed_feedback_record_fallback_call", &[(I64, &site_id)]);
                    // Guard failed: the receiver is some OTHER closure whose
                    // body codegen never saw — reset `this` here (and only
                    // here) when the static gating skipped the outer reset.
                    let fallback_prev_this = if prev_this.is_none() {
                        Some(ctx.block().call(
                            DOUBLE,
                            "js_implicit_this_set",
                            &[(DOUBLE, &undef_this)],
                        ))
                    } else {
                        None
                    };
                    let runtime_fn = format!("js_closure_call{}", lowered_args.len());
                    let mut fallback_args: Vec<(crate::types::LlvmType, &str)> =
                        vec![(I64, &closure_handle)];
                    for v in &lowered_args {
                        fallback_args.push((DOUBLE, v.as_str()));
                    }
                    let fallback_value = ctx.block().call(DOUBLE, &runtime_fn, &fallback_args);
                    if let Some(prev) = &fallback_prev_this {
                        let _ = ctx
                            .block()
                            .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, prev)]);
                    }
                    let after_fallback = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }

                    ctx.current_block = merge_idx;
                    let merged = ctx.block().phi(
                        DOUBLE,
                        &[
                            (fast_value.as_str(), after_fast.as_str()),
                            (fallback_value.as_str(), after_fallback.as_str()),
                        ],
                    );
                    if let Some(prev) = &prev_this {
                        let _ = ctx
                            .block()
                            .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, prev)]);
                    }
                    return Ok(Some(merged));
                }
            }
            // Generic js_closure_callN dispatch (unknown func id, rest
            // params, or arity mismatch): the runtime-resolved callee may
            // read `this`, so the reset is unconditional here.
            let prev_this =
                ctx.block()
                    .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &undef_this)]);
            let runtime_fn = format!("js_closure_call{}", lowered_args.len());
            let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
            for v in &lowered_args {
                call_args.push((DOUBLE, v.as_str()));
            }
            let result = ctx.block().call(DOUBLE, &runtime_fn, &call_args);
            let _ = ctx
                .block()
                .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev_this)]);
            return Ok(Some(result));
        }
    }
    Ok(None)
}
