//! Which loop holds the element base of `this.f` (DESIGN-p4l1 rev 2 §4.3,
//! PR-D; REVIEW-p4l1 findings 10 and 11). The base's emission (install,
//! refresh, reads) is `expr::index_get::field_base`.
//!
//! A loop L holds the base of field `f` iff:
//! 1. a trusted read of `this.f` **dominates L's back edge** (b3's
//!    condition (a)): every path through L's body that reaches the latch,
//!    a `continue` included, runs one first. A trusted read is a field read
//!    call (b1) on `this` whose read a base can serve
//!    (`field_base::servable`): `Arr`, dense, with a floor;
//! 2. L is the outermost such loop: an enclosing loop that holds `f`'s
//!    base already serves L, and L installs nothing for `f`;
//! 3. **admission:** every region ender in L (body and controls, nested
//!    loops included) is a back-edge poll, with the trusted reads lowered
//!    as reads. A read outside its call's floor reports its cold arm
//!    (`ColdArmCall`), which re-derives every held base; such a loop is
//!    admitted only while no address other than a field base is held. Any
//!    other call, any store, allocation, coercion of an unproven operand,
//!    `try`, `throw` or suspension refuses L, so L writes no field and
//!    grows no Array.
//!
//! **Install point:** L's guarded preheader. A `while` or a `for` with a
//! test lowers the test once more before the loop (it must write nothing
//! and end no region) and installs only when it holds, so every install is
//! followed by at least one iteration and, by rule 1, at least one read.
//! LLVM's loop rotation duplicates the same test into the preheader, and
//! the two fold into one. A `do-while` and a `for` with no test enter their
//! body unconditionally and install unguarded.
//!
//! `PERRY_ARRAY_BASE_DIAG=1` prints one line per loop with a trusted read:
//! the function, the fields and the verdict (the install-rule probe).

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use perry_hir::{Expr, Stmt};

use crate::codegen::FactsArrRep;
use crate::collectors::{FieldBase, RegionEnder};
use crate::expr::index_get::field_base as emit;
use crate::expr::FnCtx;
use crate::types::I64;

/// The fields a loop installed, for [`end`].
#[must_use]
pub(crate) struct Installed(Vec<String>);

/// A trusted read found in a loop.
struct Read {
    call: usize,
    callee: usize,
    field: String,
    rep: FactsArrRep,
    field_read: Expr,
    /// The call is in its own floor: its read has no cold arm.
    in_floor: bool,
}

fn diag(ctx: &FnCtx<'_>, fields: &[&str], verdict: &str) {
    if std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry array base] fn={} fields={} verdict={verdict}",
            ctx.func.name,
            fields.join(",")
        );
    }
}

/// Install the bases loop L qualifies for (module docs), before L is
/// lowered. `test`: L's entry test (`while`, `for` with a condition);
/// `controls`: every expression L evaluates outside its body.
pub(crate) fn begin(
    ctx: &mut FnCtx<'_>,
    test: Option<&Expr>,
    controls: &[&Expr],
    body: &[Stmt],
) -> Result<Installed> {
    if !emit::enabled()
        || ctx.wpfacts_off
        || crate::expr::typed_feedback_emission_enabled()
        || ctx.block().is_terminated()
    {
        return Ok(Installed(Vec::new()));
    }
    let mut reads = Vec::new();
    for s in body {
        reads_in_stmt(ctx, s, &mut reads);
    }
    for c in controls {
        reads_in_expr(ctx, c, &mut reads);
    }
    if reads.is_empty() {
        return Ok(Installed(Vec::new()));
    }
    let mut by_field: BTreeMap<&str, Vec<&Read>> = BTreeMap::new();
    for r in &reads {
        by_field.entry(r.field.as_str()).or_default().push(r);
    }
    let names: Vec<&str> = by_field.keys().copied().collect();
    // Rule 2: a field an enclosing loop holds is served by it.
    let held: HashSet<&str> = names
        .iter()
        .copied()
        .filter(|f| ctx.receiver_descriptors.field_base(f).is_some())
        .collect();
    // Rule 1.
    let install: Vec<&str> = names
        .iter()
        .copied()
        .filter(|f| !held.contains(f))
        .filter(|f| {
            let calls: HashSet<usize> = by_field[f].iter().map(|r| r.call).collect();
            stmts_read(body, &|e: &Expr| calls.contains(&addr(e))) == Paths::READ
        })
        .collect();
    if install.is_empty() {
        diag(
            ctx,
            &names,
            if held.is_empty() {
                "refused:no-dominating-read"
            } else {
                "served-by-outer"
            },
        );
        return Ok(Installed(Vec::new()));
    }
    // Rule 3: the trusted reads are those a base serves, here or outside.
    let served: Vec<&Read> = reads
        .iter()
        .filter(|r| install.contains(&r.field.as_str()) || held.contains(r.field.as_str()))
        .collect();
    let classified = |e: &Expr| -> Option<Option<RegionEnder>> {
        let a = addr(e);
        served.iter().find_map(|r| {
            if r.call == a {
                Some((!r.in_floor).then_some(RegionEnder::ColdArmCall))
            } else if r.callee == a {
                Some(None)
            } else {
                None
            }
        })
    };
    let is_inert = |e: &Expr| crate::rooting::expr_is_inert_primitive(ctx, e);
    let enders = crate::collectors::region_enders_in_stmts_classified(
        body,
        controls,
        &is_inert,
        &classified,
    );
    let other = enders
        .iter()
        .find(|e| !matches!(e, RegionEnder::BackEdgePoll | RegionEnder::ColdArmCall));
    let cold = enders.contains(&RegionEnder::ColdArmCall);
    if !emit::sabotaged("arrbaseadm") {
        if let Some(e) = other {
            diag(ctx, &install, &format!("refused:ender:{e:?}"));
            return Ok(Installed(Vec::new()));
        }
        if cold && !ctx.receiver_descriptors.only_field_bases() {
            diag(ctx, &install, "refused:cold-arm-under-another-address");
            return Ok(Installed(Vec::new()));
        }
        if test.is_some_and(writes) {
            diag(ctx, &install, "refused:test-writes");
            return Ok(Installed(Vec::new()));
        }
    }
    let bases: Vec<FieldBase> = install
        .iter()
        .map(|f| {
            let r = by_field[f][0];
            FieldBase {
                field: r.field.clone(),
                read: r.field_read.clone(),
                rep: r.rep,
                base_slot: ctx.func.alloca_entry(I64),
            }
        })
        .collect();
    // The guarded preheader.
    let join = test
        .map(|t| -> Result<usize> {
            let holds = crate::lower_conditional::lower_test(ctx, t)?;
            let at = ctx.new_block("abase.install");
            let join = ctx.new_block("abase.entered");
            let (at_l, join_l) = (ctx.block_label(at), ctx.block_label(join));
            ctx.block().cond_br(&holds, &at_l, &join_l);
            ctx.current_block = at;
            Ok(join)
        })
        .transpose()?;
    for base in &bases {
        emit::emit_install(ctx, base)?;
    }
    if let Some(join) = join {
        let join_l = ctx.block_label(join);
        ctx.block().br(&join_l);
        ctx.current_block = join;
    }
    let mut fields = Vec::new();
    for base in bases {
        let f = base.field.clone();
        if ctx.receiver_descriptors.materialize_field_base(base) {
            fields.push(f);
        }
    }
    let shown: Vec<&str> = fields.iter().map(String::as_str).collect();
    diag(
        ctx,
        &shown,
        if cold {
            "installed:cold-arm"
        } else {
            "installed"
        },
    );
    Ok(Installed(fields))
}

/// End the loop's bases (after the loop is lowered).
pub(crate) fn end(ctx: &mut FnCtx<'_>, installed: Installed) {
    for f in installed.0 {
        ctx.receiver_descriptors.dematerialize_field_base(&f);
    }
}

fn addr(e: &Expr) -> usize {
    e as *const Expr as usize
}

/// The trusted reads in `e` (not inside closures, which are other frames).
fn reads_in_expr(ctx: &FnCtx<'_>, e: &Expr, out: &mut Vec<Read>) {
    if matches!(e, Expr::Closure { .. }) {
        return;
    }
    if let Expr::Call { callee, args, .. } = e {
        if let Some(read) =
            crate::lower_call::field_read_call::field_read_call_at(ctx, callee, args)
        {
            if let Some(s) = emit::servable(ctx, &read) {
                let site = match callee.as_ref() {
                    Expr::PropertyGet { byte_offset, .. } => *byte_offset,
                    _ => 0,
                };
                let in_floor = ctx
                    .wpfacts
                    .field_read_calls
                    .get(&site)
                    .is_some_and(|f| f.in_floor.is_some());
                out.push(Read {
                    call: addr(e),
                    callee: addr(callee),
                    field: s.field.to_string(),
                    rep: s.rep,
                    field_read: s.field_read.clone(),
                    in_floor,
                });
            }
        }
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| reads_in_expr(ctx, c, out));
}

fn reads_in_stmt(ctx: &FnCtx<'_>, s: &Stmt, out: &mut Vec<Read>) {
    match s {
        Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e))
        | Stmt::Let { init: Some(e), .. } => reads_in_expr(ctx, e, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            reads_in_expr(ctx, condition, out);
            for st in then_branch.iter().chain(else_branch.iter().flatten()) {
                reads_in_stmt(ctx, st, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            reads_in_expr(ctx, condition, out);
            for st in body {
                reads_in_stmt(ctx, st, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                reads_in_stmt(ctx, i, out);
            }
            for e in condition.iter().chain(update.iter()) {
                reads_in_expr(ctx, e, out);
            }
            for st in body {
                reads_in_stmt(ctx, st, out);
            }
        }
        Stmt::Labeled { body, .. } => reads_in_stmt(ctx, body, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for st in body
                .iter()
                .chain(catch.iter().flat_map(|c| c.body.iter()))
                .chain(finally.iter().flatten())
            {
                reads_in_stmt(ctx, st, out);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            reads_in_expr(ctx, discriminant, out);
            for c in cases {
                if let Some(t) = &c.test {
                    reads_in_expr(ctx, t, out);
                }
                for st in &c.body {
                    reads_in_stmt(ctx, st, out);
                }
            }
        }
        _ => {}
    }
}

/// Whether the loop test `e` may write a binding or a property (it is
/// lowered twice: once as the preheader's guard).
fn writes(e: &Expr) -> bool {
    let mut found = matches!(
        e,
        Expr::LocalSet(..)
            | Expr::Update { .. }
            | Expr::PropertySet { .. }
            | Expr::PropertyUpdate { .. }
            | Expr::IndexSet { .. }
            | Expr::IndexUpdate { .. }
            | Expr::GlobalSet(..)
    );
    if !found {
        perry_hir::walker::walk_expr_children(e, &mut |c| found |= writes(c));
    }
    found
}

/// What the paths through some code do before they reach the loop's latch
/// (rule 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Paths {
    /// Some path leaves the code normally, with no trusted read yet.
    pub(crate) unread_falls: bool,
    /// Some path reaches the latch (a `continue`) with no trusted read.
    pub(crate) unread_latch: bool,
}

impl Paths {
    /// Every path has read, or left the loop.
    pub(crate) const READ: Paths = Paths {
        unread_falls: false,
        unread_latch: false,
    };
    const FALLS: Paths = Paths {
        unread_falls: true,
        unread_latch: false,
    };
    const LATCH: Paths = Paths {
        unread_falls: false,
        unread_latch: true,
    };
    fn or(self, o: Paths) -> Paths {
        Paths {
            unread_falls: self.unread_falls || o.unread_falls,
            unread_latch: self.unread_latch || o.unread_latch,
        }
    }
}

/// Rule 1 over a loop body: `READ` iff every path to the latch runs a
/// trusted read first (the end of the body is the latch).
pub(crate) fn stmts_read(body: &[Stmt], trusted: &dyn Fn(&Expr) -> bool) -> Paths {
    let p = seq(body, trusted);
    Paths {
        unread_falls: false,
        unread_latch: p.unread_latch || p.unread_falls,
    }
}

fn seq(stmts: &[Stmt], trusted: &dyn Fn(&Expr) -> bool) -> Paths {
    let mut out = Paths::FALLS;
    for s in stmts {
        if !out.unread_falls {
            break;
        }
        let p = stmt_read(s, trusted);
        out = Paths {
            unread_falls: p.unread_falls,
            unread_latch: out.unread_latch || p.unread_latch,
        };
    }
    out
}

fn stmt_read(s: &Stmt, trusted: &dyn Fn(&Expr) -> bool) -> Paths {
    match s {
        Stmt::Expr(e) | Stmt::Let { init: Some(e), .. } => {
            if expr_reads(e, trusted) {
                Paths::READ
            } else {
                Paths::FALLS
            }
        }
        // Leaves the loop: no latch on this path.
        Stmt::Return(_) | Stmt::Throw(_) | Stmt::Break => Paths::READ,
        Stmt::Continue => Paths::LATCH,
        // A labeled jump may land anywhere, the latch included.
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => Paths::LATCH,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            if expr_reads(condition, trusted) {
                return Paths::READ;
            }
            let t = seq(then_branch, trusted);
            let e = else_branch
                .as_ref()
                .map_or(Paths::FALLS, |b| seq(b, trusted));
            t.or(e)
        }
        Stmt::Labeled { body, .. } => stmt_read(body, trusted),
        // A nested loop may run its body zero times; its own `break` and
        // `continue` stay inside it. Only its first test counts.
        Stmt::While { condition, body } => {
            if expr_reads(condition, trusted) {
                Paths::READ
            } else {
                nested(body)
            }
        }
        Stmt::For {
            init,
            condition,
            body,
            ..
        } => {
            let first = init
                .as_deref()
                .is_some_and(|i| stmt_read(i, trusted) == Paths::READ)
                || condition.as_ref().is_some_and(|c| expr_reads(c, trusted));
            if first {
                Paths::READ
            } else {
                nested(body)
            }
        }
        Stmt::DoWhile { body, .. } => nested(body),
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            if expr_reads(discriminant, trusted) {
                Paths::READ
            } else {
                Paths {
                    unread_falls: true,
                    unread_latch: cases.iter().any(|c| jumps(&c.body, true)),
                }
            }
        }
        Stmt::Try { .. } => Paths {
            unread_falls: true,
            unread_latch: true,
        },
        _ => Paths::FALLS,
    }
}

/// A nested loop's body: unread fall-through, and the latch only through a
/// labeled jump.
fn nested(body: &[Stmt]) -> Paths {
    Paths {
        unread_falls: true,
        unread_latch: jumps(body, false),
    }
}

/// Whether `stmts` may jump to the latch: a labeled jump anywhere, and a
/// plain `continue` when `plain` (outside a nested loop).
fn jumps(stmts: &[Stmt], plain: bool) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        Stmt::Continue => plain,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => jumps(then_branch, plain) || else_branch.as_ref().is_some_and(|b| jumps(b, plain)),
        Stmt::Labeled { body, .. } => jumps(std::slice::from_ref(body.as_ref()), plain),
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
            jumps(body, false)
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| jumps(&c.body, plain)),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            jumps(body, plain)
                || catch.as_ref().is_some_and(|c| jumps(&c.body, plain))
                || finally.as_ref().is_some_and(|f| jumps(f, plain))
        }
        _ => false,
    })
}

/// Whether evaluating `e` always runs a trusted read: the node itself, or
/// a child every evaluation reaches. Only the forms whose children all run
/// are walked (`&&`, `||`, `??` only through their left operand, `?:`
/// through its test); any other form is "no".
fn expr_reads(e: &Expr, trusted: &dyn Fn(&Expr) -> bool) -> bool {
    if trusted(e) {
        return true;
    }
    match e {
        Expr::Logical { left, .. } => expr_reads(left, trusted),
        Expr::Conditional { condition, .. } => expr_reads(condition, trusted),
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            expr_reads(left, trusted) || expr_reads(right, trusted)
        }
        Expr::Unary { operand, .. } => expr_reads(operand, trusted),
        Expr::LocalSet(_, value) => expr_reads(value, trusted),
        Expr::Sequence(items) => items.iter().any(|x| expr_reads(x, trusted)),
        Expr::Call { callee, args, .. } => {
            expr_reads(callee, trusted) || args.iter().any(|a| expr_reads(a, trusted))
        }
        Expr::IndexGet { object, index, .. } => {
            expr_reads(object, trusted) || expr_reads(index, trusted)
        }
        Expr::PropertyGet { object, .. } => expr_reads(object, trusted),
        _ => false,
    }
}
