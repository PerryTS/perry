//! Whole-module pre-pass that finds user functions eligible for the
//! inline-hot-small heuristic: **small** functions that are called from a
//! **loop** and have **few total call sites**.
//!
//! Why all three conditions (see `codegen/function.rs` for the size gate, and
//! `linker.rs` for the raised `-inlinehint-threshold`):
//!
//! * **In a loop** (approx. "hot", since AOT has no profile) — a function only
//!   ever called from straight-line / cold code is never hinted, so cold
//!   utility functions can't bloat the binary.
//! * **Few call sites** — this is the anti-bloat backstop. `inlinehint` +
//!   the raised threshold lifts LLVM's inline ceiling for the callee at *every*
//!   one of its call sites (LLVM can't tell hot from cold per-site through a
//!   function attribute). A small function called from 1 hot loop **and** 300
//!   cold sites would therefore be duplicated 300×. Capping total call sites
//!   bounds the duplication: a hinted function is inlined at most
//!   `max_call_sites` times, so the added code is bounded regardless of the
//!   raised threshold. A bit-mixer kernel like `mix` has 1 HIR call site, so it
//!   qualifies; a shared helper called from dozens of sites does not.
//!
//! Direction of error: **under-inclusion is safe** (a missed call site just
//! forgoes an inlining opportunity or, via undercount, leaves a function
//! eligible when it has slightly more sites than counted — still bounded). We
//! never propagate the in-loop flag into a nested closure body (the closure
//! runs at its own, unknown frequency), so we can't wrongly mark a cold callee
//! hot. The walker covers the common expression containers and uses a
//! non-recursing catch-all for the long tail of runtime-intrinsic variants.

use std::collections::{HashMap, HashSet};

use perry_hir::{CallArg, Expr, Module, Stmt};

#[derive(Default)]
struct HotCalleeScan {
    /// FuncIds with ≥1 direct call site (`FuncRef` callee) inside a loop.
    in_loop: HashSet<u32>,
    /// Total direct call-site count per FuncId (loop and non-loop).
    call_counts: HashMap<u32, u32>,
}

/// Collect the set of `FuncId`s eligible for `inlinehint`: those with ≥1 direct
/// call site inside a loop AND at most `max_call_sites` total direct call sites
/// across the whole module (`init` + every function / constructor / method).
pub fn collect_hot_loop_callees(hir: &Module, max_call_sites: u32) -> HashSet<u32> {
    let mut scan = HotCalleeScan::default();
    walk_stmts(&hir.init, false, &mut scan);
    for f in &hir.functions {
        walk_stmts(&f.body, false, &mut scan);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            walk_stmts(&ctor.body, false, &mut scan);
        }
        for m in &c.methods {
            walk_stmts(&m.body, false, &mut scan);
        }
    }
    scan.in_loop
        .iter()
        .copied()
        .filter(|id| scan.call_counts.get(id).copied().unwrap_or(0) <= max_call_sites)
        .collect()
}

fn record_callee(callee: &Expr, in_loop: bool, scan: &mut HotCalleeScan) {
    if let Expr::FuncRef(id) = callee {
        *scan.call_counts.entry(*id).or_insert(0) += 1;
        if in_loop {
            scan.in_loop.insert(*id);
        }
    }
}

fn walk_stmts(stmts: &[Stmt], in_loop: bool, scan: &mut HotCalleeScan) {
    for s in stmts {
        walk_stmt(s, in_loop, scan);
    }
}

fn walk_stmt(s: &Stmt, in_loop: bool, scan: &mut HotCalleeScan) {
    match s {
        Stmt::Let { init: Some(e), .. } => walk_expr(e, in_loop, scan),
        Stmt::Let { init: None, .. } => {}
        Stmt::Expr(e) | Stmt::Throw(e) => walk_expr(e, in_loop, scan),
        Stmt::Return(Some(e)) => walk_expr(e, in_loop, scan),
        Stmt::Return(None) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_expr(condition, in_loop, scan);
            walk_stmts(then_branch, in_loop, scan);
            if let Some(eb) = else_branch {
                walk_stmts(eb, in_loop, scan);
            }
        }
        // The dominant guard: everything inside a loop body / its per-iteration
        // condition + update is "hot". The `for`-init runs once, so it keeps
        // the enclosing `in_loop`.
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            walk_expr(condition, true, scan);
            walk_stmts(body, true, scan);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                walk_stmt(init_stmt, in_loop, scan);
            }
            if let Some(c) = condition {
                walk_expr(c, true, scan);
            }
            if let Some(u) = update {
                walk_expr(u, true, scan);
            }
            walk_stmts(body, true, scan);
        }
        Stmt::Labeled { body, .. } => walk_stmt(body, in_loop, scan),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            walk_stmts(body, in_loop, scan);
            if let Some(c) = catch {
                walk_stmts(&c.body, in_loop, scan);
            }
            if let Some(f) = finally {
                walk_stmts(f, in_loop, scan);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            walk_expr(discriminant, in_loop, scan);
            for c in cases {
                if let Some(t) = &c.test {
                    walk_expr(t, in_loop, scan);
                }
                walk_stmts(&c.body, in_loop, scan);
            }
        }
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

fn walk_call_args(args: &[CallArg], in_loop: bool, scan: &mut HotCalleeScan) {
    for a in args {
        match a {
            CallArg::Expr(e) | CallArg::Spread(e) => walk_expr(e, in_loop, scan),
        }
    }
}

fn walk_expr(e: &Expr, in_loop: bool, scan: &mut HotCalleeScan) {
    match e {
        // The two forms that resolve to a known local function. Record the
        // callee (count + in-loop flag), then keep descending — args can
        // themselves contain calls / closures.
        Expr::Call { callee, args, .. } => {
            record_callee(callee, in_loop, scan);
            walk_expr(callee, in_loop, scan);
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            record_callee(callee, in_loop, scan);
            walk_expr(callee, in_loop, scan);
            walk_call_args(args, in_loop, scan);
        }

        // A closure introduces its own (unknown) invocation frequency: a loop
        // that merely *creates* a closure does not make the closure body hot.
        // Reset `in_loop` so calls inside the closure are only hot relative to
        // loops nested within the closure itself.
        Expr::Closure { body, .. } => walk_stmts(body, false, scan),

        // Assignment / update carriers.
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => walk_expr(value, in_loop, scan),

        // Arithmetic / logical / comparison trees.
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            walk_expr(left, in_loop, scan);
            walk_expr(right, in_loop, scan);
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk_expr(operand, in_loop, scan),

        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            walk_expr(condition, in_loop, scan);
            walk_expr(then_expr, in_loop, scan);
            walk_expr(else_expr, in_loop, scan);
        }

        // Member / index access + writes.
        Expr::PropertyGet { object, .. } | Expr::PropertyUpdate { object, .. } => {
            walk_expr(object, in_loop, scan)
        }
        Expr::PropertySet { object, value, .. } => {
            walk_expr(object, in_loop, scan);
            walk_expr(value, in_loop, scan);
        }
        Expr::IndexGet { object, index } => {
            walk_expr(object, in_loop, scan);
            walk_expr(index, in_loop, scan);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            walk_expr(object, in_loop, scan);
            walk_expr(index, in_loop, scan);
            walk_expr(value, in_loop, scan);
        }
        Expr::IndexUpdate { object, index, .. } => {
            walk_expr(object, in_loop, scan);
            walk_expr(index, in_loop, scan);
        }

        // Method-call forms: not `FuncRef` callees, but their receiver/args can
        // hold hot calls or closures.
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                walk_expr(o, in_loop, scan);
            }
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }
        Expr::StaticMethodCall { args, .. } => {
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }

        // Aggregates.
        Expr::Array(elements) => {
            for el in elements {
                walk_expr(el, in_loop, scan);
            }
        }
        Expr::ArraySpread(elements) => {
            for el in elements {
                match el {
                    perry_hir::ArrayElement::Expr(e) | perry_hir::ArrayElement::Spread(e) => {
                        walk_expr(e, in_loop, scan)
                    }
                    perry_hir::ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                walk_expr(v, in_loop, scan);
            }
        }
        Expr::Sequence(es) => {
            for e in es {
                walk_expr(e, in_loop, scan);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                walk_expr(a, in_loop, scan);
            }
        }

        // Everything else (literals, refs, and the long tail of runtime
        // intrinsics) can't reach a hot `FuncRef` call in the patterns this
        // heuristic targets; not descending is a safe under-approximation.
        _ => {}
    }
}
