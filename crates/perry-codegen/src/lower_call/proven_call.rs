//! FVA: a call through a value is a call of the bodies the value is.
//!
//! When the whole-program facts prove the callee set of a call
//! (`ElisionFacts::local_bodies` for a local, `FactsField::fns` for a field
//! of the receiver's proven classes), the call needs no closure/proxy test,
//! no arity test and no dispatch ladder: the runtime would only re-derive
//! what the facts say. Every body in the set is plain (no rest parameter, no
//! `arguments`), so the runtime's own plain call (`info.code(closure, this,
//! args…)`, `closure::dispatch::calln`) is the call:
//!
//! * one body: a direct call of its symbol (a closure's
//!   `perry_closure_<prefix>__<id>`, a declaration's `__perry_wrap_*`
//!   value wrapper), the arguments padded with `undefined` to its
//!   parameters or cut to them (it reads no `arguments`);
//! * several bodies: one load of the callee's `info` word, one of the body's
//!   `code` word, and an indirect call with the arguments padded to the
//!   widest declaration (over-application is safe on native ABIs, which is
//!   why a wasm target keeps the dispatcher).
//!
//! A set holding anything else has no facts here and keeps the complete
//! generic call; nothing is guarded. A contract build first asks
//! `perry_wpfacts_check_call` (a live function object of plain form) and
//! compares the returned info with the proven bodies' infos.

use anyhow::Result;
use perry_hir::Expr;

use crate::codegen::FactsBody;
use crate::expr::body_call::{emit_js_body_call, JsBody, JS_THIS_UNDEFINED};
use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I64, PTR};

/// Whether the target's ABI lets a call pass more arguments than a body
/// declares (wasm's `call_indirect` checks the signature).
fn native_abi(ctx: &FnCtx<'_>) -> bool {
    !ctx.target_triple.contains("wasm")
}

/// The symbol of one body of this module.
fn body_symbol(ctx: &FnCtx<'_>, b: &FactsBody) -> Option<String> {
    if b.closure {
        Some(crate::fn_info::closure_body_symbol(
            &ctx.strings.module_prefix(),
            b.func_id,
        ))
    } else {
        ctx.func_names
            .get(&b.func_id)
            .map(|f| format!("__perry_wrap_{f}"))
    }
}

/// `f(args)` where the local `f` holds only the bodies `local_bodies` names.
pub(crate) fn try_lower_proven_value_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::LocalGet(id) = callee else {
        return Ok(None);
    };
    if ctx.wpfacts_off || !native_abi(ctx) || ctx.local_func_ref_ids.contains_key(id) {
        return Ok(None);
    }
    // A local bound once to a closure keeps the closure arm's direct call
    // (with its typed variants) where the arm makes one; any other call of
    // it (an argument count other than its parameters) dispatches there.
    if ctx.guard_free_closure_bindings.contains(id)
        && super::early_branches::direct_closure_body(ctx, *id, args.len()).is_some()
    {
        return Ok(None);
    }
    let Some(bodies) = ctx.wpfacts.local_bodies.get(id).cloned() else {
        return Ok(None);
    };
    let Some(symbols) = bodies
        .iter()
        .map(|b| body_symbol(ctx, b))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    // The callee is evaluated before the arguments and outlives them; each
    // argument outlives the ones after it (the closure-typed local arm's
    // windows, `early_branches.rs`).
    let mut group = crate::rooting::open_rooted_group(1 + args.len());
    let callee_box = lower_expr(ctx, callee)?;
    let collects = crate::rooting::any_operand_may_collect(ctx, args.iter());
    let callee_root = group.adopt(ctx, callee, &callee_box, collects);
    let mut roots = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let collects = crate::rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
        roots.push(group.lower(ctx, a, collects)?);
    }
    let callee_box = group.reread(ctx, callee_root)?;
    let mut lowered = Vec::with_capacity(args.len());
    for r in roots {
        lowered.push(group.reread(ctx, r)?);
    }
    let v = emit_proven_call(
        ctx,
        &bodies,
        &symbols,
        &callee_box,
        JS_THIS_UNDEFINED,
        &lowered,
    );
    group.release(ctx);
    Ok(Some(v))
}

/// `o.k(args)` where `o`'s proven classes hold `k` in a definite data field
/// holding only bodies (`FactsField::fns`): the read rule's one load of the
/// slot, then the call of those bodies with `this` = `o`.
pub(crate) fn try_lower_proven_field_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return Ok(None);
    };
    if ctx.wpfacts_off || !native_abi(ctx) {
        return Ok(None);
    }
    let Some(p) = crate::expr::wpfacts_access::prove(
        ctx,
        object,
        property,
        crate::expr::wpfacts_access::Access::Get,
    ) else {
        return Ok(None);
    };
    // A guarded or nullish receiver keeps its test, and so the generic call.
    if p.guarded() || p.nullable {
        return Ok(None);
    }
    let mut bodies: Vec<FactsBody> = Vec::new();
    for &cid in &p.class_ids {
        match ctx.wpfacts.field(cid, property) {
            Some(f) if !f.fns.is_empty() => bodies.extend(f.fns.iter().cloned()),
            _ => return Ok(None),
        }
    }
    bodies.sort();
    bodies.dedup();
    let Some(symbols) = bodies
        .iter()
        .map(|b| body_symbol(ctx, b))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    // The receiver is evaluated first and is `this`; the method is read
    // before the arguments (ECMA-262 13.3.6.1), here a slot no argument can
    // write (the field is readonly or its writers are the census's), but
    // the value read is rooted across the arguments all the same.
    let mut group = crate::rooting::open_rooted_group(2 + args.len());
    let collects = crate::rooting::any_operand_may_collect(ctx, args.iter());
    let recv_root = group.lower(ctx, object, collects)?;
    let recv_box = group.reread(ctx, recv_root)?;
    let slot = crate::expr::wpfacts_access::emit_proof_slot_ptr(ctx, &recv_box, &p);
    let callee_box = ctx.block().load(DOUBLE, &slot);
    let callee_root = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &callee_box, collects);
    let mut roots = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let collects = crate::rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
        roots.push(group.lower(ctx, a, collects)?);
    }
    let recv_box = group.reread(ctx, recv_root)?;
    let callee_box = group.reread_emitted(ctx, callee_root);
    let mut lowered = Vec::with_capacity(args.len());
    for r in roots {
        lowered.push(group.reread(ctx, r)?);
    }
    let this_bits = ctx.block().bitcast_double_to_i64(&recv_box);
    let v = emit_proven_call(ctx, &bodies, &symbols, &callee_box, &this_bits, &lowered);
    group.release(ctx);
    Ok(Some(v))
}

/// `x.k(args)` where the local `x` holds exactly class C's class object and
/// the facts prove C's own static `k` holds its declaration (FVA 3.4): the
/// static call `C.k(args)`, whose lowering then needs no guard. The receiver
/// is still evaluated (a TDZ read throws).
pub(crate) fn try_lower_proven_static_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return Ok(None);
    };
    let Expr::LocalGet(id) = object.as_ref() else {
        return Ok(None);
    };
    if ctx.wpfacts_off {
        return Ok(None);
    }
    let Some(&cid) = ctx.wpfacts.local_classes.get(id) else {
        return Ok(None);
    };
    let Some(class_name) = crate::expr::wpfacts_access::class_name_of(ctx, cid) else {
        return Ok(None);
    };
    let declared = ctx
        .classes
        .get(&class_name)
        .is_some_and(|c| c.static_methods.iter().any(|m| m.name == *property));
    if !declared || !crate::expr::wpfacts_access::proves_static(ctx, cid, property) {
        return Ok(None);
    }
    let _ = lower_expr(ctx, object)?;
    let call = Expr::StaticMethodCall {
        class_name,
        method_name: property.clone(),
        args: args.to_vec(),
    };
    lower_expr(ctx, &call).map(Some)
}

/// The call of the proven bodies `bodies` (symbols `symbols`) through the
/// function object `callee_box`, receiver `this_bits`, over lowered
/// arguments.
fn emit_proven_call(
    ctx: &mut FnCtx<'_>,
    bodies: &[FactsBody],
    symbols: &[String],
    callee_box: &str,
    this_bits: &str,
    args: &[String],
) -> String {
    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let width = match bodies {
        [b] => b.params as usize,
        _ => bodies
            .iter()
            .map(|b| b.params as usize)
            .max()
            .unwrap_or(0)
            .max(args.len()),
    };
    let mut call_args: Vec<String> = args.iter().take(width).cloned().collect();
    call_args.resize(width, undefined);
    if crate::codegen::helpers::check_contracts() {
        emit_call_witness(ctx, symbols, callee_box, width);
    }
    let handle = unbox_to_i64(ctx.block(), callee_box);
    if let [sym] = symbols {
        return emit_js_body_call(
            ctx.block(),
            JsBody::Symbol(sym),
            &handle,
            this_bits,
            &call_args,
        );
    }
    let info_offset = crate::target_layout::closure_info_offset_bytes(ctx.target_triple);
    let blk = ctx.block();
    let info_addr = blk.add(I64, &handle, &info_offset.to_string());
    let info_ptr = blk.inttoptr(I64, &info_addr);
    let info = blk.load(PTR, &info_ptr);
    // `JsFunctionInfo::code` is its first field.
    let code = blk.load(PTR, &info);
    emit_js_body_call(blk, JsBody::Pointer(&code), &handle, this_bits, &call_args)
}

/// The contract witness of one elided call: the runtime validates the
/// callee and its call form and returns its info, which must be one of the
/// proven bodies' infos.
fn emit_call_witness(ctx: &mut FnCtx<'_>, symbols: &[String], callee_box: &str, argc: usize) {
    let site = ctx.wpfacts_sites as i64;
    ctx.wpfacts_sites += 1;
    let info = ctx.block().call(
        PTR,
        "perry_wpfacts_check_call",
        &[
            (DOUBLE, callee_box),
            (I64, &argc.to_string()),
            (I64, &site.to_string()),
        ],
    );
    let mut hit: Option<String> = None;
    for sym in symbols {
        let blk = ctx.block();
        let want = blk.fn_info_ref(sym);
        let eq = blk.icmp_eq(PTR, &info, &want);
        hit = Some(match hit {
            Some(h) => blk.or(I1, &h, &eq),
            None => eq,
        });
    }
    let Some(hit) = hit else { return };
    let miss_idx = ctx.new_block("proven_call.witness_miss");
    let ok_idx = ctx.new_block("proven_call.witness_ok");
    let miss_label = ctx.block_label(miss_idx);
    let ok_label = ctx.block_label(ok_idx);
    ctx.block().cond_br(&hit, &ok_label, &miss_label);
    ctx.current_block = miss_idx;
    let _ = ctx.block().call(
        crate::types::I32,
        "perry_wpfacts_check_call_miss",
        &[(PTR, &info), (I64, &site.to_string())],
    );
    ctx.block().br(&ok_label);
    ctx.current_block = ok_idx;
}
