//! Value records across typed calls (DESIGN-p4l4 §5.2, VR-3b): the record
//! `$sig` ABI.
//!
//! A signature slot the facts give `Record` carries the record as its lanes:
//!
//! - a record parameter of `F$sig` is one register per lane: the presence
//!   lane (a boolean word) when the record may be `undefined`, then one per
//!   layout word, a plain `double` for a Number field and the NaN-boxed
//!   immediate for the others (§1.2: every lane is a valid JSValue word).
//!   LLVM passes the first lanes in registers and the rest in the caller's
//!   frame, so there is no `RECORD_SCALAR_WORDS` threshold and no pointer;
//! - a record result is returned as the presence word, its lanes written to
//!   words of the caller's frame that `F$sig` receives as its last register
//!   (`RESULT_PARAM`, an explicit `sret`: a call that may collect is a
//!   statepoint, whose result LLVM lowers as one scalar, not an aggregate).
//!
//! The `value_records` rewrite writes both sides of a call in a form that is
//! also the boxed program (its module docs); this module lowers that form
//! when the callee has a `$sig` (`RecordPlan`):
//!
//! - `sig_body`: `F$sig`'s body is F's without the entry prologue, its record
//!   parameters replaced by their lane locals;
//! - `lower_return`: `return <copy>` stores the copy's lanes to the result
//!   words and returns the presence word;
//! - `try_lower_call`: an argument in a record slot is the lanes of the copy
//!   the rewrite wrote there (`copy_lanes`), no object is born;
//! - `try_lower_agg_set`: the result lanes go to the destination's field
//!   locals; the aggregate temporary keeps `true` or `undefined`.
//!
//! B3: a call the facts name a typed call of a function with a `RecordPlan`
//! that cannot enter `F$sig` is a compile error, never the boxed entry. The
//! boxed entry of such a function (`emit_entry`) admits `undefined` in a
//! record slot (an absent record) and nothing else: any other value is a
//! false proof (rc 86 in check mode, a trap otherwise), and a function that
//! returns a record has no boxed caller at all.

use anyhow::{anyhow, bail, Result};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Param};

use super::{ElisionFacts, FactsRecRep, FactsRecordFn, FactsRecordLanes, FactsRep, FactsSignature};
use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{LlvmType, DOUBLE, I1, I32, I64, PTR};

/// How one parameter of F crosses into `F$sig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Slot {
    /// One register, as P2 passes it.
    Plain,
    /// The record's lanes: `present` (a presence lane leads), and per layout
    /// word whether it is a Number lane.
    Lanes { present: bool, numeric: Vec<bool> },
}

/// The record part of a typed plan (`SpecFnPlan::record`): the plan's
/// `signature`, `reps` and `sig_defaults` are then per lane.
#[derive(Debug, Clone)]
pub(crate) struct RecordPlan {
    pub fact: FactsRecordFn,
    /// Per parameter of F.
    pub slots: Vec<Slot>,
    /// The record `F$sig` returns.
    pub ret: Option<FactsRecRep>,
    /// The literal defaults per parameter of F (`sig_function`).
    pub defaults: Vec<Option<f64>>,
}

impl RecordPlan {
    /// The index of parameter `i`'s first lane in the lane signature.
    pub(crate) fn lane_index(&self, i: usize) -> usize {
        self.slots[..i]
            .iter()
            .map(|s| match s {
                Slot::Plain => 1,
                Slot::Lanes { present, numeric } => usize::from(*present) + numeric.len(),
            })
            .sum()
    }
}

/// The register of `F$sig` that points to the caller's result words: a
/// record result is returned through it (module docs).
pub(crate) const RESULT_PARAM: &str = "%perry_record_ret";

fn words(facts: &ElisionFacts, tmpl: u32) -> Option<usize> {
    facts.record_layouts.get(&tmpl).map(|l| l.keys.len())
}

fn undefined_lit() -> String {
    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
}

fn bool_lit(b: bool) -> String {
    double_literal(f64::from_bits(if b {
        crate::nanbox::TAG_TRUE
    } else {
        crate::nanbox::TAG_FALSE
    }))
}

/// An absent record's lane.
fn filler(numeric: bool) -> String {
    if numeric {
        double_literal(0.0)
    } else {
        undefined_lit()
    }
}

/// The lane signature of `sig` for F, and its record plan: `None` when the
/// rewrite gave F no record parameter and no record result (F is then typed
/// as P2 types it, its record slots Boxed).
pub(crate) fn expand(
    sig: &FactsSignature,
    f: &Function,
    facts: &ElisionFacts,
    defaults: &[Option<f64>],
) -> Option<(FactsSignature, RecordPlan)> {
    let fact = facts.record_fns.get(&f.id)?;
    if fact.params.len() != sig.params.len() {
        return None;
    }
    let ret = sig
        .record_ret
        .filter(|r| fact.ret && words(facts, r.tmpl).is_some());
    let mut slots = Vec::with_capacity(sig.params.len());
    let mut params = Vec::new();
    let mut ranges = Vec::new();
    for (i, lanes) in fact.params.iter().enumerate() {
        let layout = lanes
            .as_ref()
            .and_then(|l| facts.record_layouts.get(&l.tmpl).map(|layout| (l, layout)));
        match layout {
            Some((l, layout)) if layout.numeric.len() == l.fields.len() => {
                if l.present.is_some() {
                    params.push(FactsRep::Boxed);
                    ranges.push(None);
                }
                for n in &layout.numeric {
                    params.push(if *n { FactsRep::F64 } else { FactsRep::Boxed });
                    ranges.push(None);
                }
                slots.push(Slot::Lanes {
                    present: l.present.is_some(),
                    numeric: layout.numeric.clone(),
                });
            }
            _ => {
                params.push(sig.params[i]);
                ranges.push(sig.ranges[i]);
                slots.push(Slot::Plain);
            }
        }
    }
    if ret.is_none() && slots.iter().all(|s| *s == Slot::Plain) {
        return None;
    }
    Some((
        FactsSignature {
            params,
            ranges,
            ret: sig.ret,
            this_class: sig.this_class,
            zero_ok: sig.zero_ok,
            record_params: Vec::new(),
            record_ret: None,
        },
        RecordPlan {
            fact: fact.clone(),
            slots,
            ret,
            defaults: defaults.to_vec(),
        },
    ))
}

/// Probe (`PERRY_VALUE_RECORDS_TRACE=1`): a function with record lanes gets
/// no `$sig`, so its records cross boxed (a copy at each call, key reads in
/// the body).
pub(crate) fn trace_boxed(name: &str, why: &str) {
    if std::env::var("PERRY_VALUE_RECORDS_TRACE").as_deref() == Ok("1") {
        eprintln!(
            "perry: value records: {name} gets no record $sig ({why}): its records cross boxed"
        );
    }
}

/// `defaults` (per parameter of F) per lane.
pub(crate) fn lane_defaults(plan: &RecordPlan) -> Vec<Option<f64>> {
    let mut out = Vec::new();
    for (slot, d) in plan.slots.iter().zip(&plan.defaults) {
        match slot {
            Slot::Plain => out.push(*d),
            Slot::Lanes { present, numeric } => {
                out.extend(std::iter::repeat_n(
                    None,
                    usize::from(*present) + numeric.len(),
                ));
            }
        }
    }
    out
}

/// F as `F$sig` compiles it (`signature_entry::sig_function`): for a record
/// plan, also without the entry prologue of its record parameters, which are
/// the lane locals themselves.
pub(crate) fn sig_body(
    f: &Function,
    defaults: &[Option<f64>],
    record: Option<&RecordPlan>,
) -> Option<Function> {
    let Some(plan) = record else {
        return super::signature_entry::sig_function(f, defaults);
    };
    let mut g = f.clone();
    let (at, len) = (plan.fact.prologue_at, plan.fact.prologue_len);
    if g.body.len() < at + len {
        return None;
    }
    g.body.drain(at..at + len);
    let mut out = super::signature_entry::sig_function(&g, &plan.defaults)?;
    let lane = |p: &Param, id: u32, what: &str| Param {
        id,
        name: format!("{}${what}", p.name),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let mut params = Vec::new();
    for (i, p) in out.params.iter().enumerate() {
        match (&plan.slots[i], &plan.fact.params[i]) {
            (Slot::Lanes { .. }, Some(l)) => {
                if let Some(present) = l.present {
                    params.push(lane(p, present, "present"));
                }
                for (j, id) in l.fields.iter().enumerate() {
                    params.push(lane(p, *id, &format!("lane{j}")));
                }
            }
            _ => params.push(p.clone()),
        }
    }
    out.params = params;
    Some(out)
}

/// The lanes of the copy `e` the rewrite wrote for template `tmpl`
/// (`value_records::Rewriter::materialize`): `new <class>(fields…)` or the
/// object literal of the layout keys, each value a field local, under
/// `present ? … : undefined` when the binding tracks presence.
pub(crate) fn copy_lanes(facts: &ElisionFacts, tmpl: u32, e: &Expr) -> Option<FactsRecordLanes> {
    let (present, born) = match e {
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } if matches!(else_expr.as_ref(), Expr::Undefined) => match condition.as_ref() {
            Expr::LocalGet(p) => (Some(*p), then_expr.as_ref()),
            _ => return None,
        },
        _ => (None, e),
    };
    let birth = facts.record_births.get(&tmpl)?;
    let n = words(facts, tmpl)?;
    let local = |a: &Expr| match a {
        Expr::LocalGet(id) => Some(*id),
        _ => None,
    };
    let mut fields = vec![0u32; n];
    match (&birth.class, born) {
        (
            Some(name),
            Expr::New {
                class_name, args, ..
            },
        ) if class_name == name && args.len() == n && birth.order.len() == n => {
            for (j, a) in args.iter().enumerate() {
                fields[birth.order[j]] = local(a)?;
            }
        }
        (None, Expr::Object(props)) if props.len() == n => {
            let keys = &facts.record_layouts.get(&tmpl)?.keys;
            for (j, (k, v)) in props.iter().enumerate() {
                if k != &keys[j] {
                    return None;
                }
                fields[j] = local(v)?;
            }
        }
        _ => return None,
    }
    Some(FactsRecordLanes {
        tmpl,
        present,
        fields,
    })
}

/// `e` as the rewrite writes a record slot: the expressions that run first
/// (a value written into a temporary's lanes), then the copy.
fn split_copy(e: &Expr) -> (&[Expr], &Expr) {
    match e {
        Expr::Sequence(list) if !list.is_empty() => {
            let (last, first) = list.split_last().expect("a non-empty sequence");
            (first, last)
        }
        _ => (&[], e),
    }
}

/// `return e` of a body whose `$sig` returns record `r`: the lanes of the
/// copy the rewrite returns are stored to the caller's result words, and the
/// presence word is the value returned.
pub(crate) fn lower_return(ctx: &mut FnCtx<'_>, r: FactsRecRep, e: Option<&Expr>) -> Result<()> {
    let facts = ctx.wpfacts.clone();
    let n =
        words(&facts, r.tmpl).ok_or_else(|| anyhow!("ICE: value record result has no layout"))?;
    let (present, lanes): (String, Vec<String>) = match e {
        // The absent record: its words are not read.
        None => (bool_lit(false), Vec::new()),
        Some(e) => {
            let (first, copy) = split_copy(e);
            for x in first {
                lower_expr(ctx, x)?;
            }
            let Some(l) = copy_lanes(&facts, r.tmpl, copy) else {
                bail!("ICE: a value record `$sig` returns a value that is no copy of its result lanes");
            };
            let present = match l.present {
                Some(p) => lower_expr(ctx, &Expr::LocalGet(p))?,
                None => bool_lit(true),
            };
            let mut lanes = Vec::with_capacity(n);
            for id in &l.fields {
                lanes.push(lower_expr(ctx, &Expr::LocalGet(*id))?);
            }
            (present, lanes)
        }
    };
    for (j, v) in lanes.iter().enumerate() {
        let blk = ctx.block();
        let slot = blk.gep(DOUBLE, RESULT_PARAM, &[(I64, &j.to_string())]);
        blk.store(DOUBLE, v, &slot);
    }
    for _ in 0..ctx.try_depth {
        crate::stmt::try_stmt::emit_try_end(ctx);
    }
    if ctx.shared_super_scope_active {
        ctx.block().call_void("js_derived_super_scope_pop", &[]);
    }
    crate::expr::private_resume_frame::write_out_kind(ctx)?;
    crate::expr::resume_locals::write_back(ctx)?;
    ctx.block().ret(DOUBLE, &present);
    Ok(())
}

/// What a call into a record `$sig` produced.
pub(crate) enum Called {
    /// The call's value (a canonical double).
    Value(String),
    /// A record result: the presence word, and the pointer to the result
    /// words in this frame.
    Record {
        present: String,
        words: String,
        rep: FactsRecRep,
    },
}

/// The callee of the typed call at this site when its plan is a record
/// plan: `(sig name, lane signature, lane defaults, plan, receiver)`.
struct Site {
    name: String,
    sig: FactsSignature,
    defaults: Vec<Option<f64>>,
    plan: RecordPlan,
    method: bool,
    offset: u32,
}

fn site(ctx: &mut FnCtx<'_>, callee: &Expr) -> Result<Option<Site>> {
    let direct_offset = super::signature_entry::direct_call_offset();
    match callee {
        Expr::FuncRef(fid) => {
            let Some(plan) = ctx.spec_abi_functions.get(fid) else {
                return Ok(None);
            };
            let (Some(record), Some(sig)) = (plan.record.clone(), plan.signature.clone()) else {
                return Ok(None);
            };
            // A call the facts do not name a typed call passes no record
            // (wpfacts `signatures`: every source call of a Record slot's
            // body is typed): it enters the boxed entry.
            if direct_offset == 0 || ctx.wpfacts.typed_calls.get(&direct_offset) != Some(fid) {
                return Ok(None);
            }
            let Some(fname) = ctx.func_names.get(fid).cloned() else {
                bail!("ICE: a value record typed call names an unknown function");
            };
            Ok(Some(Site {
                name: super::spec_entry_name(&fname, plan),
                defaults: plan.sig_defaults.clone(),
                sig,
                plan: record,
                method: false,
                offset: direct_offset,
            }))
        }
        Expr::PropertyGet {
            object,
            byte_offset,
            ..
        } => {
            let offset = if *byte_offset != 0 {
                *byte_offset
            } else {
                direct_offset
            };
            if offset == 0 {
                return Ok(None);
            }
            let Some(&mid) = ctx.wpfacts.typed_calls.get(&offset) else {
                return Ok(None);
            };
            let Some(tm) = ctx.typed_methods.get(&mid) else {
                return Ok(None);
            };
            let (Some(record), Some(sig)) = (tm.plan.record.clone(), tm.plan.signature.clone())
            else {
                return Ok(None);
            };
            let defaults = tm.plan.sig_defaults.clone();
            let key = (tm.class.clone(), tm.name.clone());
            let Some(public) = ctx.methods.get(&key).cloned() else {
                bail!(
                    "ICE: value record typed call @{offset}: method {}::{} has no symbol",
                    key.0,
                    key.1
                );
            };
            // B3: the facts made this a typed call of one body; a receiver
            // codegen cannot prove has no boxed route for a record.
            if !matches!(
                crate::expr::wpfacts_access::proven_receiver_classes(ctx, object),
                Some((_, false))
            ) {
                bail!(
                    "ICE: value record typed call @{offset} of {}::{}: the receiver is not a proven instance",
                    key.0,
                    key.1
                );
            }
            Ok(Some(Site {
                name: format!("{public}$sig"),
                sig,
                defaults,
                plan: record,
                method: true,
                offset,
            }))
        }
        _ => Ok(None),
    }
}

/// A typed call whose callee has a record plan (module docs): `None` when
/// this call is no such call. `wpfacts_off` does not apply (§5.5: a record
/// is a representation, not a proof).
pub(crate) fn try_lower_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<Called>> {
    if ctx.wpfacts.record_fns.is_empty() {
        return Ok(None);
    }
    let Some(site) = site(ctx, callee)? else {
        return Ok(None);
    };
    let facts = ctx.wpfacts.clone();
    let offset = site.offset;
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::Signature,
        || serde_json::json!({ "call": offset, "record": true }),
    );
    crate::facts_consumed::used();
    // The receiver and the plain arguments are rooted values; a record
    // slot's lanes are immediates, read where the argument is evaluated.
    enum Arg {
        Rooted(usize),
        Lanes(Vec<String>),
    }
    let mut roots = crate::rooting::open_rooted_group(1 + args.len());
    let recv_idx = match callee {
        Expr::PropertyGet { object, .. } if site.method => {
            let collects = crate::rooting::any_operand_may_collect(ctx, args.iter());
            Some(roots.lower(ctx, object, collects)?)
        }
        _ => None,
    };
    let mut lowered: Vec<Arg> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let slot = site.plan.slots.get(i).cloned().unwrap_or(Slot::Plain);
        match slot {
            Slot::Plain => {
                let collects = crate::rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
                lowered.push(Arg::Rooted(roots.lower(ctx, a, collects)?));
            }
            Slot::Lanes { present, numeric } => {
                let tmpl = site.plan.fact.params[i]
                    .as_ref()
                    .map(|l| l.tmpl)
                    .expect("a lane slot's template");
                let (first, copy) = split_copy(a);
                for x in first {
                    lower_expr(ctx, x)?;
                }
                let mut lanes = Vec::with_capacity(numeric.len() + 1);
                if matches!(copy, Expr::Undefined) {
                    if !present {
                        bail!("ICE: value record typed call @{offset}: `undefined` in a record slot that is never absent");
                    }
                    lanes.push(bool_lit(false));
                    lanes.extend(numeric.iter().map(|n| filler(*n)));
                } else {
                    let Some(l) = copy_lanes(&facts, tmpl, copy) else {
                        bail!("ICE: value record typed call @{offset}: argument {i} is no copy of a record's lanes");
                    };
                    match (present, l.present) {
                        (true, Some(p)) => lanes.push(lower_expr(ctx, &Expr::LocalGet(p))?),
                        (true, None) => lanes.push(bool_lit(true)),
                        (false, None) => {}
                        // A temporary tracks presence; the facts say this slot's
                        // record is never absent, so the slot has no lane for it.
                        (false, Some(_)) => {}
                    }
                    for id in &l.fields {
                        lanes.push(lower_expr(ctx, &Expr::LocalGet(*id))?);
                    }
                }
                lowered.push(Arg::Lanes(lanes));
            }
        }
    }
    let mut raw: Vec<(LlvmType, String)> = Vec::with_capacity(site.sig.params.len() + 1);
    if let Some(idx) = recv_idx {
        raw.push((DOUBLE, roots.reread(ctx, idx)?));
    }
    for (i, slot) in site.plan.slots.iter().enumerate() {
        let lane = site.plan.lane_index(i);
        match (slot, lowered.get(i)) {
            (Slot::Lanes { .. }, Some(Arg::Lanes(lanes))) => {
                for (j, v) in lanes.iter().enumerate() {
                    super::signature_entry::witness_value(
                        ctx,
                        site.sig.params[lane + j],
                        None,
                        v,
                        &format!("signature argument {i} lane {j} @{offset}"),
                    );
                    raw.push((DOUBLE, v.clone()));
                }
            }
            (Slot::Lanes { present, numeric }, None) => {
                if !present {
                    bail!("ICE: value record typed call @{offset}: argument {i} is omitted in a record slot that is never absent");
                }
                raw.push((DOUBLE, bool_lit(false)));
                raw.extend(numeric.iter().map(|n| (DOUBLE, filler(*n))));
            }
            (Slot::Plain, passed) => {
                let passed = match passed {
                    Some(Arg::Rooted(idx)) => Some(roots.reread(ctx, *idx)?),
                    _ => None,
                };
                let rep = site.sig.params[lane];
                let value = match (site.defaults[lane], passed) {
                    (Some(literal), None) => double_literal(literal),
                    (Some(literal), Some(v)) => {
                        super::signature_entry::emit_default_select(ctx.block(), &v, literal)
                    }
                    (None, Some(v)) => v,
                    (None, None) => undefined_lit(),
                };
                super::signature_entry::witness_value(
                    ctx,
                    rep,
                    site.sig.ranges[lane],
                    &value,
                    &format!("signature argument {i} @{offset}"),
                );
                raw.push(match rep.encoding() {
                    super::FactsEnc::I32 => (I32, ctx.block().fptosi(DOUBLE, &value, I32)),
                    super::FactsEnc::F64 | super::FactsEnc::Boxed => (DOUBLE, value),
                });
            }
            (Slot::Lanes { .. }, Some(Arg::Rooted(_))) => unreachable!("a lane slot lowers lanes"),
        }
    }
    crate::expr::stack_guard::emit_stack_guard_before_clone_call(ctx, &site.name);
    // A record result is written to words of this frame (an entry-block
    // slot), never to memory another body reaches.
    let result_words = site.plan.ret.map(|r| {
        let n = words(&facts, r.tmpl).expect("a record plan's result layout");
        ctx.func.alloca_entry_array(DOUBLE, n.max(1))
    });
    if let Some(words) = &result_words {
        raw.push((PTR, words.clone()));
    }
    let call_args: Vec<(LlvmType, &str)> = raw.iter().map(|(t, v)| (*t, v.as_str())).collect();
    let ty = super::signature_entry::sig_llvm_ty(site.sig.ret);
    let value = ctx.block().call(ty, &site.name, &call_args);
    let out = match (site.plan.ret, result_words) {
        (Some(rep), Some(words)) => Called::Record {
            present: value,
            words,
            rep,
        },
        _ => Called::Value(if site.sig.ret == FactsRep::I32 {
            ctx.block().sitofp(I32, &value, DOUBLE)
        } else {
            value
        }),
    };
    roots.release(ctx);
    Ok(Some(out))
}

/// `agg = <typed call>` for an aggregate temporary of the rewrite
/// (`ElisionFacts::record_aggs`): the call's result lanes are bound to the
/// destination's field locals, and the temporary holds `true` (present) or
/// `undefined`. `None` when the call does not enter a record `$sig`: the
/// assignment is then the ordinary one, of a boxed result.
pub(crate) fn try_lower_agg_set(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::LocalSet(id, value) = expr else {
        return Ok(None);
    };
    if ctx.wpfacts.record_aggs.is_empty() {
        return Ok(None);
    }
    let Some(dest) = ctx.wpfacts.record_aggs.get(id).cloned() else {
        return Ok(None);
    };
    // VR-4b: a record array's `pop` or `a[i]` returns its record this way.
    let called = match super::record_array::try_lower_source(ctx, value)? {
        Some(c) => Some(c),
        None => {
            let Expr::Call {
                callee,
                args,
                byte_offset,
                ..
            } = value.as_ref()
            else {
                return Ok(None);
            };
            super::signature_entry::with_direct_call(*byte_offset, || {
                try_lower_call(ctx, callee, args)
            })?
        }
    };
    let (present, result_words, r) = match called {
        None => return Ok(None),
        Some(Called::Value(v)) => {
            crate::expr::bind_local(ctx, *id, &v, value)?;
            return Ok(Some(v));
        }
        Some(Called::Record {
            present,
            words,
            rep,
        }) => (present, words, rep),
    };
    if r.tmpl != dest.tmpl {
        bail!("ICE: a value record result is bound to another template's lanes");
    }
    let facts = ctx.wpfacts.clone();
    let keys = facts.record_layouts[&r.tmpl].keys.clone();
    // An absent result leaves its words unwritten: the lanes read then are
    // never used (every read of the destination tests its presence).
    for (j, field) in dest.fields.iter().enumerate() {
        let blk = ctx.block();
        let slot = blk.gep(DOUBLE, &result_words, &[(I64, &j.to_string())]);
        let lane = blk.load(DOUBLE, &slot);
        // The hint is the boxed program's read of this lane.
        let hint = Expr::PropertyGet {
            object: Box::new(Expr::LocalGet(*id)),
            property: keys[j].clone(),
            byte_offset: 0,
            byte_end: 0,
        };
        crate::expr::bind_local(ctx, *field, &lane, &hint)?;
    }
    let state = if r.opt {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&present);
        let is_true = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64);
        blk.select(I1, &is_true, DOUBLE, &bool_lit(true), &undefined_lit())
    } else {
        bool_lit(true)
    };
    crate::expr::bind_local(ctx, *id, &state, &Expr::Undefined)?;
    Ok(Some(state))
}

/// A typed call of a record plan in a value position: its value, when it
/// returns no record. A record result reaches lowering only through its
/// aggregate temporary (`try_lower_agg_set`).
pub(crate) fn try_lower_value_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    match try_lower_call(ctx, callee, args)? {
        None => Ok(None),
        Some(Called::Value(v)) => Ok(Some(v)),
        Some(Called::Record { .. }) => {
            bail!("ICE: a value record result of a typed call is not bound to its lanes")
        }
    }
}

/// The boxed entry's lanes for argument `arg` of a record slot (module
/// docs): `undefined` is the absent record; anything else is a false proof.
/// Returns the block to continue in and the lanes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn entry_lanes(
    wf: &mut crate::function::LlFunction,
    cur: usize,
    arg: &str,
    present: bool,
    numeric: &[bool],
    site: Option<&crate::verify_facts::SiteArg>,
) -> (usize, Vec<String>) {
    let blk = wf.block_mut(cur).unwrap();
    let bits = blk.bitcast_double_to_i64(arg);
    let undefined = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
    // A slot that is never absent admits nothing.
    let ok = if present {
        undefined
    } else {
        "false".to_string()
    };
    if site.is_some() {
        wf.block_mut(cur)
            .unwrap()
            .call_void("perry_verify_facts_tick", &[]);
    }
    let miss = wf.blocks().len();
    let miss_l = wf.create_block("signature.record_miss").label.clone();
    let next = wf.blocks().len();
    let next_l = wf.create_block("signature.record_absent").label.clone();
    wf.block_mut(cur).unwrap().cond_br(&ok, &next_l, &miss_l);
    emit_false_proof(wf.block_mut(miss).unwrap(), arg, site);
    let mut lanes = Vec::with_capacity(numeric.len() + 1);
    if present {
        lanes.push(bool_lit(false));
    }
    lanes.extend(numeric.iter().map(|n| filler(*n)));
    (next, lanes)
}

/// A record reached a boxed entry: rc 86 in check mode, a trap otherwise.
pub(crate) fn emit_false_proof(
    blk: &mut crate::block::LlBlock,
    value: &str,
    site: Option<&crate::verify_facts::SiteArg>,
) {
    if let Some(site) = site {
        let len = site.len.clone().unwrap();
        let _ = blk.call(
            I32,
            "perry_verify_facts_miss",
            &[(DOUBLE, value), (I64, &site.pointer), (I64, &len)],
        );
    } else {
        blk.call_void("llvm.trap", &[]);
    }
    blk.unreachable();
}
