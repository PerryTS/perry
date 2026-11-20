//! The element base of `this.f` held across the rest of a method body
//! (N2, DESIGN-n2-fn-bases.md): PR-D's loop region (`stmt::field_base`)
//! widened to the held suffix of a function. Same table entry, install,
//! poll refresh, cold-arm re-derivation, reads and witness
//! (`expr::index_get::field_base`).
//!
//! A method body `S0 … Sn-1` holds the base of `this.f` from `Sk` to its
//! end iff:
//! 1. **admission:** every region ender of `Sk … Sn-1` is a back-edge poll
//!    or the cold arm of a served read (PR-D's rule 3, with the trusted reads
//!    classified as PR-D classifies them). The one widening: an `if` that
//!    N3's lowering takes as an unrolled walk
//!    (`const_array_walk::unrolled_body`, the decision the lowering itself
//!    makes) contributes only the enders of its body, since nothing else of
//!    it is lowered;
//! 2. `Sk` is the first statement of the longest admitted suffix that holds
//!    a trusted read;
//! 3. the suffix holds at least two trusted read sites of `this.f`
//!    (a site in an unrolled walk counts once per copy, one in a loop twice).
//!
//! The install runs where the list lowering reaches `Sk` (`at_stmt`), and the
//! region ends where that lowering of the list ends (`end_list`): every
//! lowering of the body list (a split may lower it more than once) installs
//! its own base before the reads it serves. A unit that starts before `Sk`
//! and covers it installs nothing; its reads stay C3's.
//!
//! `PERRY_FACTS_FN_BASES=0` turns it off (PR-D's build). Negative control:
//! `PERRY_FACTS_SABOTAGE=fnbaseadm` admits a suffix whatever ends its
//! region. `PERRY_ARRAY_BASE_DIAG=1` prints the verdict per function
//! (`[perry fn base]`).

use std::collections::BTreeMap;

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use super::field_base::{self as loop_base, Read};
use crate::collectors::{FieldBase, RegionEnder};
use crate::expr::index_get::field_base as emit;
use crate::expr::FnCtx;
use crate::types::I64;

/// The planned function-level bases of one method body (`FnCtx::fn_base`).
#[derive(Default)]
pub(crate) struct Pending {
    /// The body list (address, length) the plan is for.
    list: Option<(usize, usize)>,
    /// `Sk`.
    start: usize,
    bases: Vec<FieldBase>,
    /// The fields this lowering of the list materialized.
    active: Vec<String>,
}

/// `PERRY_FACTS_FN_BASES=0` turns the function-level installer off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_FN_BASES").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn diag(ctx: &FnCtx<'_>, fields: &[&str], start: usize, n: usize, verdict: &str) {
    if std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry fn base] fn={} fields={} start={start}/{n} verdict={verdict}",
            ctx.func.name,
            fields.join(",")
        );
    }
}

fn key(stmts: &[Stmt]) -> (usize, usize) {
    (stmts.as_ptr() as usize, stmts.len())
}

/// Plan the held suffix of a method body (module docs), before the body is
/// lowered.
pub(crate) fn plan(ctx: &mut FnCtx<'_>, body: &[Stmt]) {
    ctx.fn_base = Pending::default();
    if !enabled()
        || !emit::enabled()
        || ctx.wpfacts_off
        || crate::expr::typed_feedback_emission_enabled()
    {
        return;
    }
    let mut reads = Vec::new();
    for s in body {
        loop_base::reads_in_stmt(ctx, s, &mut reads);
    }
    if reads.is_empty() {
        return;
    }
    let n = body.len();
    let names: Vec<&str> = {
        let mut v: Vec<&str> = reads.iter().map(|r| r.field.as_str()).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    // Rule 1, from the end: the longest admitted suffix.
    let admit_all = emit::sabotaged("fnbaseadm");
    let mut from = n;
    let mut cold = false;
    while from > 0 {
        let enders = stmt_enders(ctx, &body[from - 1], &reads);
        if !admit_all
            && enders
                .iter()
                .any(|e| !matches!(e, RegionEnder::BackEdgePoll | RegionEnder::ColdArmCall))
        {
            break;
        }
        cold |= enders.contains(&RegionEnder::ColdArmCall);
        from -= 1;
    }
    // Rule 2: start at the suffix's first statement with a trusted read.
    let Some(start) = (from..n).find(|&i| sites(ctx, &body[i..=i], None) > 0) else {
        diag(ctx, &names, from, n, "refused:no-read-in-admitted-suffix");
        return;
    };
    if cold && !admit_all && !ctx.receiver_descriptors.only_field_bases() {
        diag(
            ctx,
            &names,
            start,
            n,
            "refused:cold-arm-under-another-address",
        );
        return;
    }
    // Rule 3, per field.
    let mut by_field: BTreeMap<&str, &Read> = BTreeMap::new();
    for r in &reads {
        by_field.entry(r.field.as_str()).or_insert(r);
    }
    let mut bases = Vec::new();
    let mut shown = Vec::new();
    for (f, r) in by_field {
        if sites(ctx, &body[start..], Some(f)) < 2 {
            continue;
        }
        shown.push(f);
        bases.push(FieldBase {
            field: r.field.clone(),
            read: r.field_read.clone(),
            rep: r.rep,
            base_slot: ctx.func.alloca_entry(I64),
        });
    }
    if bases.is_empty() {
        diag(ctx, &names, start, n, "refused:one-site");
        return;
    }
    diag(
        ctx,
        &shown,
        start,
        n,
        if cold { "planned:cold-arm" } else { "planned" },
    );
    ctx.fn_base = Pending {
        list: Some(key(body)),
        start,
        bases,
        active: Vec::new(),
    };
}

/// Before statement `i` of a list: the install, when `i` is the planned
/// body's `Sk`.
pub(crate) fn at_stmt(ctx: &mut FnCtx<'_>, stmts: &[Stmt], i: usize) -> Result<()> {
    if ctx.fn_base.list != Some(key(stmts)) || i != ctx.fn_base.start {
        return Ok(());
    }
    if !ctx.fn_base.active.is_empty() || ctx.block().is_terminated() {
        return Ok(());
    }
    let bases = ctx.fn_base.bases.clone();
    for base in &bases {
        emit::emit_install(ctx, base)?;
    }
    for base in bases {
        let f = base.field.clone();
        if ctx.receiver_descriptors.materialize_field_base(base) {
            ctx.fn_base.active.push(f);
        }
    }
    Ok(())
}

/// After a list is lowered: the end of the region its lowering installed.
pub(crate) fn end_list(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) {
    if ctx.fn_base.list != Some(key(stmts)) {
        return;
    }
    for f in std::mem::take(&mut ctx.fn_base.active) {
        ctx.receiver_descriptors.dematerialize_field_base(&f);
    }
}

/// After the body: no plan outlives it.
pub(crate) fn finish(ctx: &mut FnCtx<'_>) {
    debug_assert!(ctx.fn_base.active.is_empty());
    ctx.fn_base = Pending::default();
}

/// The body of `s` when it is a walk N3 unrolls, with its copy count.
fn walk_body<'s>(ctx: &FnCtx<'_>, s: &'s Stmt) -> Option<(&'s [Stmt], usize)> {
    let Stmt::If {
        condition,
        then_branch,
        ..
    } = s
    else {
        return None;
    };
    super::const_array_walk::unrolled_body(ctx, condition, then_branch)
}

/// The region enders of `s` (module docs, rule 1), with every trusted read
/// in `reads` classified as PR-D classifies a served read.
fn stmt_enders(ctx: &FnCtx<'_>, s: &Stmt, reads: &[Read]) -> Vec<RegionEnder> {
    if let Some((body, _)) = walk_body(ctx, s) {
        return body
            .iter()
            .flat_map(|st| stmt_enders(ctx, st, reads))
            .collect();
    }
    let is_inert = |e: &Expr| crate::rooting::expr_is_inert_primitive(ctx, e);
    let primitive = |e: &Expr| {
        let a = loop_base::addr(e);
        reads
            .iter()
            .any(|r| r.call == a && r.rep.elem != crate::codegen::FactsElemRep::Boxed)
    };
    let classified = |e: &Expr| -> Option<Option<RegionEnder>> {
        let a = loop_base::addr(e);
        if let Some(r) = reads.iter().find(|r| r.call == a) {
            return Some((!r.in_floor).then_some(RegionEnder::ColdArmCall));
        }
        if reads.iter().any(|r| r.callee == a) {
            return Some(None);
        }
        (!is_inert(e) && loop_base::operator_over(e, &is_inert, &primitive)).then_some(None)
    };
    crate::collectors::region_enders_in_stmts_classified(
        std::slice::from_ref(s),
        &[],
        &is_inert,
        &classified,
    )
}

/// Trusted read sites of `field` (any field when `None`) in `stmts`: a site
/// in an unrolled walk once per copy, one in a loop twice (rule 3).
fn sites(ctx: &FnCtx<'_>, stmts: &[Stmt], field: Option<&str>) -> usize {
    stmts
        .iter()
        .map(|s| {
            if let Some((body, copies)) = walk_body(ctx, s) {
                return copies * sites(ctx, body, field);
            }
            let mut reads = Vec::new();
            loop_base::reads_in_stmt(ctx, s, &mut reads);
            let n = reads
                .iter()
                .filter(|r| field.is_none_or(|f| r.field == f))
                .count();
            match s {
                Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => 2 * n,
                _ => n,
            }
        })
        .sum()
}
