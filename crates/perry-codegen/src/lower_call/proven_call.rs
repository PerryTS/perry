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
//! * one body: a direct call of it, the arguments padded with `undefined`
//!   to its parameters or cut to them (it reads no `arguments`). A closure
//!   is called as the closure-typed local's direct arm calls it
//!   (`early_branches::emit_known_closure_body_call`): its typed entry
//!   behind the guards of its typed parameters, the generic body on a miss,
//!   or `perry_closure_<prefix>__<id>` when it has no typed entry for these
//!   arguments; a declaration through its `__perry_wrap_*` value wrapper;
//! * two to [`ARMS_CAP`] bodies: one load of the callee's `info` word and a
//!   direct call of the body that info names. A body has exactly one info,
//!   `@<body>$info`, defined by the module that defines the body and named
//!   by every allocation of a function object of it (`fn_info.rs`), so two
//!   closures of one body (two environments) are one arm. Each body but the
//!   last is compared with the info; the set is complete, so the last body
//!   is called without a compare and there is no other arm. Each arm is the
//!   one-body call of its body;
//! * more bodies: one load of the `info` word, one of the body's `code`
//!   word, and an indirect call with the arguments padded to the widest
//!   declaration (over-application is safe on native ABIs, which is why a
//!   wasm target keeps the dispatcher).
//!
//! A set holding anything else has no facts here and keeps the complete
//! generic call; nothing is guarded. A contract or facts-verification build
//! (`verify_facts`) first asks
//! `perry_wpfacts_check_call` (a live function object of plain form) and
//! compares the returned info with the proven bodies' infos.

use anyhow::Result;
use perry_hir::Expr;

use crate::codegen::FactsBody;
use crate::expr::body_call::{emit_js_body_call, JsBody, JS_THIS_UNDEFINED};
use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I1, I64, PTR};

/// The most bodies a proven call names arm by arm (one info compare per
/// body but the last); a larger set is one indirect call through the code
/// word.
const ARMS_CAP: usize = 4;

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

/// The bodies a call through the local `id` runs, when the facts prove the
/// local holds only them (`ElisionFacts::local_bodies`). The lowering below
/// and the LIFE census (`life_census_sides`) resolve a value call by it.
pub(crate) fn value_call_bodies(
    facts: &crate::codegen::ElisionFacts,
    id: u32,
) -> Option<&[FactsBody]> {
    facts.local_bodies.get(&id).map(Vec::as_slice)
}

/// The bodies a call of field `property` runs on a receiver of `class_ids`:
/// in every class the field holds only bodies (`FactsField::fns`), sorted
/// and without repeats. `None` when some class's field holds anything else.
/// The caller has proven the read of the field (`wpfacts_access`).
pub(crate) fn field_call_bodies(
    facts: &crate::codegen::ElisionFacts,
    class_ids: &[u32],
    property: &str,
) -> Option<Vec<FactsBody>> {
    let mut bodies: Vec<FactsBody> = Vec::new();
    for &cid in class_ids {
        match facts.field(cid, property) {
            Some(f) if !f.fns.is_empty() => {
                crate::facts_consumed::used();
                bodies.extend(f.fns.iter().cloned())
            }
            _ => return None,
        }
    }
    bodies.sort();
    bodies.dedup();
    Some(bodies)
}

/// `f(args)` where the local `f` holds only the bodies `local_bodies` names.
pub(crate) fn try_lower_proven_value_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    if let Some(value) = crate::stmt::numeric_callbacks::try_lower(ctx, callee, args)? {
        return Ok(Some(value));
    }
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
    let Some(bodies) = value_call_bodies(&ctx.wpfacts, *id).map(<[FactsBody]>::to_vec) else {
        return Ok(None);
    };
    crate::facts_consumed::used();
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
        args,
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
    let Some(bodies) = field_call_bodies(&ctx.wpfacts, &p.class_ids, property) else {
        return Ok(None);
    };
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
    let v = emit_proven_call(
        ctx,
        &bodies,
        &symbols,
        &callee_box,
        &this_bits,
        &lowered,
        args,
    );
    group.release(ctx);
    Ok(Some(v))
}

/// `x.k(args)` where the local `x` holds only class objects of class C and
/// the facts prove C's own static `k` holds its declaration (FVA 3.4): a
/// direct call of that declaration, with no guard.
///
/// The receiver stays the call's `this`. The facts' class object of C stands
/// for every evaluation of C's declaration: a declaration that runs more than
/// once (in a function body, #11759 (c′)) gives each evaluation its own class
/// object, with its own statics and its own class environment, which the body
/// reaches through `this` (`js_static_this_resolve_class`). A call of
/// `C.k(args)` would bind the first evaluation's.
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
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ClassAlias,
        || serde_json::json!({"local":id,"class":cid}),
    );
    crate::facts_consumed::used();
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
    let key = (
        class_name.clone(),
        crate::codegen::static_method_registry_key(property),
    );
    let Some(fn_name) = ctx.methods.get(&key).cloned() else {
        return Ok(None);
    };
    // A rest parameter or a generator body takes the general static call
    // lowering (argument bundling, generator state).
    let generator = ctx.classes.get(&class_name).is_some_and(|c| {
        c.static_methods.iter().any(|m| {
            m.name == *property && crate::codegen::helpers::body_has_fixed_generator_state(&m.body)
        })
    });
    if generator || ctx.method_has_rest.get(&key).copied().unwrap_or(false) {
        return Ok(None);
    }
    let declared = ctx.method_param_counts.get(&key).copied().unwrap_or(0);
    crate::rooting::with_rooted_group(ctx, 1, |ctx, group| {
        // The receiver is evaluated first (a TDZ read throws) and held across
        // the arguments.
        let collects = crate::rooting::any_operand_may_collect(ctx, args.iter());
        let recv_idx = group.lower(ctx, object, collects)?;
        let (mut lowered, args_group) = super::lower_call_args_rooted(ctx, args)?;
        // Missing arguments are `undefined` (default parameters fire); extra
        // ones are evaluated and dropped.
        let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        lowered.resize(declared, undefined);
        let recv = group.reread(ctx, recv_idx)?;
        ctx.block()
            .call_void("js_static_this_arm_value", &[(DOUBLE, &recv)]);
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|v| (DOUBLE, v.as_str())).collect();
        let result = ctx.block().call(DOUBLE, &fn_name, &arg_slices);
        args_group.release(ctx);
        Ok(Some(result))
    })
}

/// The call of the proven bodies `bodies` (symbols `symbols`) through the
/// function object `callee_box`, receiver `this_bits`, over lowered
/// arguments `args` (`hir_args` their HIR).
fn emit_proven_call(
    ctx: &mut FnCtx<'_>,
    bodies: &[FactsBody],
    symbols: &[String],
    callee_box: &str,
    this_bits: &str,
    args: &[String],
    hir_args: &[Expr],
) -> String {
    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    // The arguments as a call of a body declaring `width` parameters
    // receives them: cut to its parameters (it reads no `arguments`) or
    // padded with `undefined`.
    let fit = |width: usize| -> Vec<String> {
        let mut call_args: Vec<String> = args.iter().take(width).cloned().collect();
        call_args.resize(width, undefined.clone());
        call_args
    };
    let width = match bodies {
        [b] => b.params as usize,
        _ => bodies
            .iter()
            .map(|b| b.params as usize)
            .max()
            .unwrap_or(0)
            .max(args.len()),
    };
    if crate::verify_facts::witnesses() {
        crate::verify_facts::bodies_checked(bodies, width);
        emit_call_witness(ctx, symbols, callee_box, width);
    }
    let handle = unbox_to_i64(ctx.block(), callee_box);
    // The call of one known body: a closure's through its typed entry
    // where it has one for these arguments (the closure-typed local's
    // direct arm, decision 129), a declaration's value wrapper directly.
    let body_call = |ctx: &mut FnCtx<'_>, b: &FactsBody, sym: &str| -> String {
        let call_args = fit(b.params as usize);
        if b.closure {
            super::early_branches::emit_known_closure_body_call(
                ctx, b.func_id, sym, &handle, this_bits, &call_args, hir_args,
            )
        } else {
            emit_js_body_call(
                ctx.block(),
                JsBody::Symbol(sym),
                &handle,
                this_bits,
                &call_args,
            )
        }
    };
    if let ([b], [sym]) = (bodies, symbols) {
        return body_call(ctx, b, sym);
    }
    let info_offset = crate::target_layout::closure_info_offset_bytes(ctx.target_triple);
    let blk = ctx.block();
    let info_addr = blk.add(I64, &handle, &info_offset.to_string());
    let info_ptr = blk.inttoptr(I64, &info_addr);
    let info = blk.load(PTR, &info_ptr);
    if symbols.len() > ARMS_CAP {
        // `JsFunctionInfo::code` is its first field.
        let code = blk.load(PTR, &info);
        return emit_js_body_call(blk, JsBody::Pointer(&code), &handle, this_bits, &fit(width));
    }
    // The body the info names: every body but the last is compared; the
    // last is what the info is when no compare matched.
    let join_idx = ctx.new_block("proven_call.join");
    let join_label = ctx.block_label(join_idx);
    let last = symbols.len() - 1;
    let mut incoming: Vec<(String, String)> = Vec::with_capacity(symbols.len());
    for (i, (b, sym)) in bodies.iter().zip(symbols).enumerate() {
        if i < last {
            let arm_idx = ctx.new_block("proven_call.arm");
            let next_idx = ctx.new_block("proven_call.next");
            let arm_label = ctx.block_label(arm_idx);
            let next_label = ctx.block_label(next_idx);
            let blk = ctx.block();
            let want = blk.fn_info_ref(sym);
            let is = blk.icmp_eq(PTR, &info, &want);
            blk.cond_br(&is, &arm_label, &next_label);
            ctx.current_block = arm_idx;
            let v = body_call(ctx, b, sym);
            incoming.push((v, ctx.block().label.clone()));
            ctx.block().br(&join_label);
            ctx.current_block = next_idx;
        } else {
            let v = body_call(ctx, b, sym);
            incoming.push((v, ctx.block().label.clone()));
            ctx.block().br(&join_label);
        }
    }
    ctx.current_block = join_idx;
    let incoming: Vec<(&str, &str)> = incoming
        .iter()
        .map(|(v, l)| (v.as_str(), l.as_str()))
        .collect();
    ctx.block().phi(DOUBLE, &incoming)
}

/// The contract witness of one elided call: the runtime validates the
/// callee and its call form and returns its info, which must be one of the
/// proven bodies' infos.
fn emit_call_witness(ctx: &mut FnCtx<'_>, symbols: &[String], callee_box: &str, argc: usize) {
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("proven call bodies={symbols:?}"),
        crate::verify_facts::Fact::Bodies,
        &[],
    );
    let info = crate::verify_facts::check(
        ctx,
        PTR,
        "perry_wpfacts_check_call",
        &[(DOUBLE, callee_box), (I64, &argc.to_string())],
        &site,
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
    let _ = crate::verify_facts::check(
        ctx,
        crate::types::I32,
        "perry_wpfacts_check_call_miss",
        &[(PTR, &info)],
        &site,
    );
    ctx.block().br(&ok_label);
    ctx.current_block = ok_idx;
}
