//! P2 (DESIGN-p2-typed-signatures rev 2.3 §3–§5): typed functions.
//!
//! A function is typed when wpfacts gives it a non-Boxed `Signature`, at
//! least one same-module `typed_calls` site reaches it, and no excluded tier
//! selected it (`select`, computed once after every selector ran as on
//! main). A typed function has exactly two symbols:
//!
//! - `F$sig`, the one body: the ordinary `compile_function` through the
//!   spec_abi path, with each parameter register in its signature
//!   representation (I32 an `i32`, F64 a double, Boxed as today), the
//!   stack check, and a raw return (an `i32` for an I32 `ret`);
//! - the public `F`, the boxed entry (`emit_boxed_entry`): it applies the
//!   literal defaults (§4.1), decodes each argument (§4.2), tail-calls
//!   `F$sig`, and returns a canonical double (R4).
//!
//! A `typed_calls` site whose callee is typed calls `F$sig` directly
//! (`try_emit_typed_call`). Every subsumed tier (spec_abi tuples,
//! typed_abi `typed_f64`/`typed_i32`) is dropped for typed functions only;
//! the budget slot they free is not refilled (T3).

use std::collections::HashSet;

use perry_hir::{CompareOp, Expr, Function, Stmt};

use super::opts::CrossModuleCtx;
use super::{FactsRep, FactsSignature, SpecDispatch, SpecFnPlan};
use crate::block::LlBlock;
use crate::collectors::SpecParamRep;
use crate::expr::FnCtx;
use crate::module::LlModule;
use crate::nanbox::double_literal;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64};
use crate::verify_facts::Fact;

thread_local! {
    /// The byte offset of the direct call being lowered (`with_direct_call`).
    static DIRECT_CALL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Lower direct call `f` with its own byte offset visible to
/// `try_emit_typed_call` (the `typed_calls` key is the call's HIR offset).
pub(crate) fn with_direct_call<R>(byte_offset: u32, f: impl FnOnce() -> R) -> R {
    let saved = DIRECT_CALL.with(|s| s.replace(byte_offset));
    let out = f();
    DIRECT_CALL.with(|s| s.set(saved));
    out
}

fn literal_number(e: &Expr) -> Option<f64> {
    match e {
        Expr::Number(n) => Some(*n),
        Expr::Integer(i) => Some(*i as f64),
        _ => None,
    }
}

/// `if (p === undefined) { p = <default>; }`, the prologue statement HIR
/// lowering prepends for a defaulted parameter `p`.
fn is_default_prologue(s: &Stmt, id: u32) -> bool {
    let Stmt::If {
        condition:
            Expr::Compare {
                op: CompareOp::Eq,
                left,
                right,
            },
        then_branch,
        else_branch: None,
    } = s
    else {
        return false;
    };
    matches!(left.as_ref(), Expr::LocalGet(x) if *x == id)
        && matches!(right.as_ref(), Expr::Undefined)
        && matches!(then_branch.as_slice(), [Stmt::Expr(Expr::LocalSet(x, _))] if *x == id)
}

/// F as `F$sig` compiles it: a parameter whose literal default the caller
/// applies (§4.1) has no default and no prologue statement. `None` when the
/// body does not start with the prologue HIR lowering emits.
pub(crate) fn sig_function(f: &Function, defaults: &[Option<f64>]) -> Option<Function> {
    let mut out = f.clone();
    let mut at = 0usize;
    let mut drop: Vec<usize> = Vec::new();
    for (p, d) in f.params.iter().zip(defaults.iter()) {
        if p.default.is_none() {
            continue;
        }
        if !is_default_prologue(f.body.get(at)?, p.id) {
            return None;
        }
        if d.is_some() {
            drop.push(at);
        }
        at += 1;
    }
    for i in drop.into_iter().rev() {
        out.body.remove(i);
    }
    for (p, d) in out.params.iter_mut().zip(defaults.iter()) {
        if d.is_some() {
            p.default = None;
        }
    }
    Some(out)
}

/// The typed plan of `f`, or `None` when `f` is not typed (§3.2, T2).
fn typed_plan(
    f: &Function,
    sig: &FactsSignature,
    cm: &CrossModuleCtx,
    func_synthetic_arguments: &HashSet<u32>,
    triple: &str,
) -> Option<SpecFnPlan> {
    if sig.params.len() != f.params.len()
        || (sig.ret == FactsRep::Boxed && sig.params.iter().all(|r| *r == FactsRep::Boxed))
    {
        return None;
    }
    // Shapes the spec_abi path cannot compile, and §1's exclusions.
    if f.is_async
        || f.is_generator
        || f.was_plain_async
        || !f.captures.is_empty()
        || f.params
            .iter()
            .any(|p| p.is_rest || p.arguments_object.is_some())
        || func_synthetic_arguments.contains(&f.id)
        || super::split_regions::is_region_function(f)
    {
        return None;
    }
    // Excluded tiers (§3.2.1): typed_i1, typed_string, `$this`, arena, a
    // spec tuple with TaPtr/NumberArray, integer_contract.
    if cm.typed_i1_functions.contains(&f.id)
        || cm.typed_string_functions.contains(&f.id)
        || cm.funcs_reading_dynamic_this.contains(&f.id)
        || cm.arena_threaded_functions.contains(&f.id)
    {
        return None;
    }
    if let Some(plan) = cm.spec_abi_functions.get(&f.id) {
        if plan
            .reps
            .iter()
            .any(|r| matches!(r, SpecParamRep::TaPtr { .. } | SpecParamRep::NumberArray))
        {
            return None;
        }
        if plan.reps.iter().all(|r| matches!(r, SpecParamRep::Boxed))
            && super::checked_integer::eligible(f, triple)
            && super::integer_contract::prove(f).is_some()
        {
            return None;
        }
    }
    let mut defaults = Vec::with_capacity(f.params.len());
    for (p, rep) in f.params.iter().zip(sig.params.iter()) {
        defaults.push(match (&p.default, rep) {
            (Some(d), FactsRep::I32 | FactsRep::F64) => Some(literal_number(d)?),
            _ => None,
        });
    }
    let body = sig_function(f, &defaults)?;
    let demoted = crate::collectors::callee_demoted_params(&body);
    let reps = sig
        .params
        .iter()
        .zip(demoted.iter())
        .map(|(rep, demoted)| match (rep, demoted) {
            (FactsRep::I32, false) => SpecParamRep::I32,
            (FactsRep::F64, false) => SpecParamRep::F64,
            _ => SpecParamRep::Boxed,
        })
        .collect();
    Some(SpecFnPlan {
        reps,
        dispatch: SpecDispatch::Static,
        guards: vec![None; f.params.len()],
        signature: Some(sig.clone()),
        sig_defaults: defaults,
    })
}

/// Compute `typed_fns` once (T2) and install it: each typed function's plan
/// replaces its spec_abi plan, and its subsumed tiers are dropped (T3).
/// Runs after every tier selector, before any body or site is lowered.
pub(crate) fn select(
    hir: &perry_hir::Module,
    cm: &mut CrossModuleCtx,
    func_synthetic_arguments: &HashSet<u32>,
    triple: &str,
) {
    let facts = cm.wpfacts.clone();
    if facts.interp_guard || facts.signatures.is_empty() {
        return;
    }
    let reached: HashSet<u32> = facts.typed_calls.values().copied().collect();
    let mut typed: Vec<(u32, SpecFnPlan)> = Vec::new();
    for f in &hir.functions {
        if !reached.contains(&f.id) {
            continue;
        }
        let Some(sig) = facts.signatures.get(&f.id) else {
            continue;
        };
        if let Some(plan) = typed_plan(f, sig, cm, func_synthetic_arguments, triple) {
            typed.push((f.id, plan));
        }
    }
    for (id, plan) in typed {
        cm.typed_f64_functions.remove(&id);
        cm.typed_i32_functions.remove(&id);
        cm.spec_return_proofs.remove(&id);
        cm.spec_abi_functions.insert(id, plan);
    }
}

/// i1: double `value` is a value of `rep` within `range` (§5). `None` for
/// Boxed, which claims nothing.
fn emit_rep_test(
    blk: &mut LlBlock,
    rep: FactsRep,
    range: Option<(i64, i64)>,
    value: &str,
) -> Option<String> {
    match rep {
        FactsRep::Boxed => None,
        FactsRep::F64 => Some(super::typed_entry::emit_plain_number_test(blk, value)),
        FactsRep::I32 => {
            let (lo, hi) = range.unwrap_or((i32::MIN as i64, i32::MAX as i64));
            let (lo, hi) = (lo.max(i32::MIN as i64), hi.min(i32::MAX as i64));
            let ge = blk.fcmp("oge", value, &double_literal(lo as f64));
            let le = blk.fcmp("ole", value, &double_literal(hi as f64));
            let in_range = blk.and(I1, &ge, &le);
            let safe = blk.select(I1, &in_range, DOUBLE, value, "0.0");
            let int = blk.fptosi(DOUBLE, &safe, I32);
            let back = blk.sitofp(I32, &int, DOUBLE);
            let exact = blk.fcmp("oeq", &back, value);
            let bits = blk.bitcast_double_to_i64(value);
            let not_negative_zero = blk.icmp_ne(I64, &bits, "-9223372036854775808");
            let ok = blk.and(I1, &in_range, &exact);
            Some(blk.and(I1, &ok, &not_negative_zero))
        }
    }
}

/// Check mode: `value` is in `rep`/`range` at a lowered site; a miss exits 86.
fn witness_value(
    ctx: &mut FnCtx<'_>,
    rep: FactsRep,
    range: Option<(i64, i64)>,
    value: &str,
    what: &str,
) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let Some(ok) = emit_rep_test(ctx.block(), rep, range, value) else {
        return;
    };
    crate::verify_facts::covered(Fact::Signature, || serde_json::json!({ "what": what }));
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("signature.verify_miss");
    let next = ctx.new_block("signature.admitted");
    let miss_l = ctx.block_label(miss);
    let next_l = ctx.block_label(next);
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(ctx, what, Fact::Signature, &[]);
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, value)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

/// Check mode: a value `F$sig` returns is in its `ret` (§5).
pub(crate) fn witness_return(ctx: &mut FnCtx<'_>, rep: FactsRep, value: &str) {
    witness_value(ctx, rep, None, value, "signature return");
}

/// The LLVM type of a signature slot in `F$sig`.
pub(crate) fn sig_llvm_ty(rep: FactsRep) -> LlvmType {
    match rep {
        FactsRep::I32 => I32,
        FactsRep::F64 | FactsRep::Boxed => DOUBLE,
    }
}

/// `arg === undefined ? L : arg` (§4.1).
fn emit_default_select(blk: &mut LlBlock, arg: &str, literal: f64) -> String {
    let bits = blk.bitcast_double_to_i64(arg);
    let undefined = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
    blk.select(I1, &undefined, DOUBLE, &double_literal(literal), arg)
}

/// A typed call site (§2): `typed_calls` names this call's offset, its
/// callee `fid` is typed, and this body may use the facts. Each argument
/// takes its signature representation (an omitted literal default is the
/// literal; I32 is one `fptosi` of the proven double), is witnessed in
/// check mode, and the call enters `F$sig`. An I32 result is `sitofp`.
pub(crate) fn try_emit_typed_call(
    ctx: &mut FnCtx<'_>,
    fid: u32,
    fname: &str,
    args: &[Expr],
    lowered: &[String],
) -> Option<String> {
    let offset = DIRECT_CALL.with(|s| s.get());
    if offset == 0
        || ctx.wpfacts_off
        || ctx.wpfacts.interp_guard
        || ctx.wpfacts.typed_calls.get(&offset) != Some(&fid)
    {
        return None;
    }
    let plan = ctx.spec_abi_functions.get(&fid)?;
    let sig = plan.signature.clone()?;
    let defaults = plan.sig_defaults.clone();
    if lowered.len() < sig.params.len() {
        return None;
    }
    crate::verify_facts::consumed(Fact::Signature, || serde_json::json!({ "call": offset }));
    crate::facts_consumed::used();
    let mut raw: Vec<(LlvmType, String)> = Vec::with_capacity(sig.params.len());
    for (i, rep) in sig.params.iter().enumerate() {
        let value = match (defaults[i], args.get(i)) {
            (Some(literal), None) => double_literal(literal),
            (Some(literal), Some(_)) => emit_default_select(ctx.block(), &lowered[i], literal),
            (None, _) => lowered[i].clone(),
        };
        witness_value(
            ctx,
            *rep,
            sig.ranges[i],
            &value,
            &format!("signature argument {i} @{offset}"),
        );
        raw.push(match rep {
            FactsRep::I32 => (I32, ctx.block().fptosi(DOUBLE, &value, I32)),
            FactsRep::F64 | FactsRep::Boxed => (DOUBLE, value),
        });
    }
    let sig_name = super::spec_entry_name(fname, plan);
    crate::expr::stack_guard::emit_stack_guard_before_clone_call(ctx, &sig_name);
    let call_args: Vec<(LlvmType, &str)> = raw.iter().map(|(t, v)| (*t, v.as_str())).collect();
    let ret_ty = sig_llvm_ty(sig.ret);
    let value = ctx.block().call(ret_ty, &sig_name, &call_args);
    Some(if sig.ret == FactsRep::I32 {
        ctx.block().sitofp(I32, &value, DOUBLE)
    } else {
        value
    })
}

/// The boxed entry: the public `F` of a typed function (§3.3). Each argument
/// takes its literal default when it is `undefined` (§4.1), is decoded to
/// its parameter's representation (§4.2: `emit_entry_number`, then one
/// `fptosi` for I32), is witnessed in check mode (R2), and the entry
/// tail-calls `F$sig`; an I32 result goes back as a canonical double (R4).
pub(crate) fn emit_boxed_entry(
    llmod: &mut LlModule,
    f: &Function,
    public_name: &str,
    plan: &SpecFnPlan,
) {
    let sig = plan.signature.as_ref().expect("a typed plan");
    let params: Vec<(LlvmType, String)> = f
        .params
        .iter()
        .map(|p| (DOUBLE, format!("%arg{}", p.id)))
        .collect();
    let sig_name = super::spec_entry_name(public_name, plan);
    let check = crate::verify_facts::enabled();
    let sites: Vec<Option<crate::verify_facts::SiteArg>> = sig
        .params
        .iter()
        .enumerate()
        .map(|(i, rep)| {
            (check && *rep != FactsRep::Boxed).then(|| {
                crate::verify_facts::raw_entry_site(
                    public_name,
                    public_name,
                    Fact::Signature,
                    &format!("boxed entry argument {i}"),
                )
            })
        })
        .collect();
    let wf = llmod.define_function(public_name, DOUBLE, params);
    let _ = wf.create_block("entry");
    let mut cur = 0usize;
    let mut raw: Vec<(LlvmType, String)> = Vec::with_capacity(f.params.len());
    for (i, p) in f.params.iter().enumerate() {
        let arg = format!("%arg{}", p.id);
        let rep = sig.params[i];
        let blk = wf.block_mut(cur).unwrap();
        let value = match plan.sig_defaults[i] {
            Some(literal) => emit_default_select(blk, &arg, literal),
            None => arg,
        };
        if rep == FactsRep::Boxed {
            raw.push((DOUBLE, value));
            continue;
        }
        let value = super::facts_number_locals::emit_entry_number(blk, &value);
        if let Some(site) = &sites[i] {
            crate::verify_facts::covered(Fact::Signature, || {
                serde_json::json!({ "entry": public_name, "argument": i })
            });
            let ok = emit_rep_test(blk, rep, sig.ranges[i], &value).unwrap();
            blk.call_void("perry_verify_facts_tick", &[]);
            let miss = wf.blocks().len();
            let miss_l = wf.create_block("signature.entry_miss").label.clone();
            let next = wf.blocks().len();
            let next_l = wf.create_block("signature.entry_admitted").label.clone();
            wf.block_mut(cur).unwrap().cond_br(&ok, &next_l, &miss_l);
            let mb = wf.block_mut(miss).unwrap();
            let len = site.len.clone().unwrap();
            let _ = mb.call(
                I32,
                "perry_verify_facts_miss",
                &[(DOUBLE, &value), (I64, &site.pointer), (I64, &len)],
            );
            mb.unreachable();
            cur = next;
        }
        let blk = wf.block_mut(cur).unwrap();
        raw.push(match rep {
            FactsRep::I32 => (I32, blk.fptosi(DOUBLE, &value, I32)),
            _ => (DOUBLE, value),
        });
    }
    let call_args: Vec<(LlvmType, &str)> = raw.iter().map(|(t, v)| (*t, v.as_str())).collect();
    let blk = wf.block_mut(cur).unwrap();
    let value = blk.call(sig_llvm_ty(sig.ret), &sig_name, &call_args);
    let value = if sig.ret == FactsRep::I32 {
        blk.sitofp(I32, &value, DOUBLE)
    } else {
        value
    };
    blk.ret(DOUBLE, &value);
}

/// `F$sig`'s parameters that seed `nonnegative_integer_locals` (T1): an I32
/// parameter in its i32 slot whose interval has `lo >= 0` (the slot is
/// never written: plan selection demoted written parameters).
pub(crate) fn nonnegative_params(f: &Function, plan: &SpecFnPlan) -> HashSet<u32> {
    let Some(sig) = plan.signature.as_ref() else {
        return HashSet::new();
    };
    f.params
        .iter()
        .zip(plan.reps.iter())
        .zip(sig.ranges.iter())
        .filter(|((_, rep), range)| {
            matches!(rep, SpecParamRep::I32) && range.is_some_and(|(lo, _)| lo >= 0)
        })
        .map(|((p, _), _)| p.id)
        .collect()
}
