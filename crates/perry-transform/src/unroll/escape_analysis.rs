//! #2308 escaping-id analysis for the static-loop unroller.
//!
//! Splits out the pass that decides which loop-body-declared locals are
//! shared bindings and so must keep their original id across unrolled copies
//! rather than being renamed per copy by `refresh_local_ids`. See
//! `compute_loop_escaping_ids`.

use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// #2308 / #6876: compute the set of loop-body-declared local ids that name
/// shared bindings. This includes:
///
/// - ids referenced outside the loop body; and
/// - ids also declared outside the loop body.
///
/// The second case is how lowered, hoisted `var` declarations are represented:
/// one `Stmt::Let` creates the function-scoped slot at entry and the declaration
/// in the source loop reuses that same id. Such a `var` still has to keep its
/// value across iterations even when every read is inside the loop. Looking at
/// references alone missed that case and refreshed the declaration to a new id
/// per unrolled copy.
///
/// The unroller must NOT rename shared ids: every unrolled copy has to use the
/// same slot, matching JavaScript `var` semantics. A block-scoped `let`/`const`
/// declared in a loop body has neither an outside declaration nor a legal
/// outside reference, so it keeps getting fresh per-copy ids (preserving
/// distinct closure captures).
///
/// Computed on the ORIGINAL (un-unrolled) body so reference counts are
/// stable. For references and declarations independently, `total > inside`
/// means the loop-body declaration participates in a binding visible outside
/// that body and must be protected.
pub(super) fn compute_loop_escaping_ids(stmts: &[Stmt]) -> HashSet<LocalId> {
    let mut total_refs: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmts(stmts, &mut total_refs);
    let mut total_decls: HashMap<LocalId, usize> = HashMap::new();
    count_declared_ids_stmts(stmts, &mut total_decls);
    let mut escaping = HashSet::new();
    collect_escaping_in_stmts(stmts, &total_refs, &total_decls, &mut escaping);
    escaping
}

fn collect_escaping_in_stmts(
    stmts: &[Stmt],
    total_refs: &HashMap<LocalId, usize>,
    total_decls: &HashMap<LocalId, usize>,
    escaping: &mut HashSet<LocalId>,
) {
    for s in stmts {
        if let Stmt::For { body, .. } = s {
            let mut inside_refs: HashMap<LocalId, usize> = HashMap::new();
            count_local_refs_stmts(body, &mut inside_refs);
            let mut decls: HashSet<LocalId> = HashSet::new();
            collect_declared_ids_stmts(body, &mut decls);
            for id in decls {
                let refs_escape = total_refs.get(&id).copied().unwrap_or(0)
                    > inside_refs.get(&id).copied().unwrap_or(0);
                let declaration_is_shared = total_decls.get(&id).copied().unwrap_or(0) > 1;
                if refs_escape || declaration_is_shared {
                    escaping.insert(id);
                }
            }
        }
        // Recurse into every nested stmt list so inner loops are analyzed
        // against the same whole-scope `total` reference counts.
        each_child_stmt_list(s, &mut |list| {
            collect_escaping_in_stmts(list, total_refs, total_decls, escaping)
        });
    }
}

/// Collect every `Stmt::Let`-declared id in `stmts` (recursing into nested
/// blocks, including for-loop `init`). Closure params / catch params are
/// not `Stmt::Let` and can never escape a loop, so they're skipped.
fn collect_declared_ids_stmts(stmts: &[Stmt], out: &mut HashSet<LocalId>) {
    for s in stmts {
        if let Stmt::Let { id, .. } = s {
            out.insert(*id);
        }
        if let Stmt::For { init, .. } = s {
            if let Some(init_stmt) = init {
                if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                    out.insert(*id);
                }
            }
        }
        each_child_stmt_list(s, &mut |list| collect_declared_ids_stmts(list, out));
    }
}

/// Count declarations rather than collecting unique ids. A repeated id is the
/// key signal for a lowered hoisted `var`: its function-entry declaration and
/// its source-position declaration intentionally address the same slot.
fn count_declared_ids_stmts(stmts: &[Stmt], out: &mut HashMap<LocalId, usize>) {
    for s in stmts {
        if let Stmt::Let { id, .. } = s {
            *out.entry(*id).or_insert(0) += 1;
        }
        if let Stmt::For { init, .. } = s {
            if let Some(init_stmt) = init {
                if let Stmt::Let { id, .. } = init_stmt.as_ref() {
                    *out.entry(*id).or_insert(0) += 1;
                }
            }
        }
        each_child_stmt_list(s, &mut |list| count_declared_ids_stmts(list, out));
    }
}

/// Invoke `f` on each nested `&[Stmt]` directly owned by `stmt` (then/else
/// arms, loop/switch/try bodies, labeled body). Used by the #2308
/// escaping-id analysis to recurse without duplicating the stmt match.
fn each_child_stmt_list<F: FnMut(&[Stmt])>(stmt: &Stmt, f: &mut F) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            f(then_branch);
            if let Some(eb) = else_branch {
                f(eb);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => f(body),
        Stmt::Switch { cases, .. } => {
            for c in cases {
                f(&c.body);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            f(body);
            if let Some(c) = catch {
                f(&c.body);
            }
            if let Some(fin) = finally {
                f(fin);
            }
        }
        Stmt::Labeled { body, .. } => each_child_stmt_list(body, f),
        _ => {}
    }
}

/// Count local-id USE sites (reads/writes), NOT declarations, across
/// `stmts`. Mirrors the id-bearing variants of `scan_expr_for_max_local`
/// but accumulates per-id counts so the #2308 analysis can tell whether an
/// id is used outside a given loop body.
///
/// Also reused by the generator's #6345 per-iteration analysis
/// (`generator::per_iteration`), which needs the same "is this id used
/// outside the loop that declares it?" question answered. Kept in one place
/// so a newly-added id-bearing `Expr` variant only has to be taught to
/// `count_local_refs_expr` once — missing one would silently under-count and
/// let a `var` be mistaken for a block-scoped binding.
pub(crate) fn count_local_refs_stmts(stmts: &[Stmt], counts: &mut HashMap<LocalId, usize>) {
    for s in stmts {
        count_local_refs_stmt(s, counts);
    }
}

pub(crate) fn count_local_refs_stmt(stmt: &Stmt, counts: &mut HashMap<LocalId, usize>) {
    match stmt {
        // A `Stmt::Let` id is a DECLARATION, not a use — don't count it;
        // only walk its initializer for uses.
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                count_local_refs_expr(e, counts);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => count_local_refs_expr(e, counts),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                count_local_refs_expr(e, counts);
            }
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            count_local_refs_expr(condition, counts);
            count_local_refs_stmts(then_branch, counts);
            if let Some(eb) = else_branch {
                count_local_refs_stmts(eb, counts);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            count_local_refs_expr(condition, counts);
            count_local_refs_stmts(body, counts);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                count_local_refs_stmt(init_stmt, counts);
            }
            if let Some(c) = condition {
                count_local_refs_expr(c, counts);
            }
            if let Some(u) = update {
                count_local_refs_expr(u, counts);
            }
            count_local_refs_stmts(body, counts);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            count_local_refs_stmts(body, counts);
            if let Some(c) = catch {
                count_local_refs_stmts(&c.body, counts);
            }
            if let Some(f) = finally {
                count_local_refs_stmts(f, counts);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            count_local_refs_expr(discriminant, counts);
            for c in cases {
                if let Some(t) = &c.test {
                    count_local_refs_expr(t, counts);
                }
                count_local_refs_stmts(&c.body, counts);
            }
        }
        Stmt::Labeled { body, .. } => count_local_refs_stmt(body, counts),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

pub(crate) fn count_local_refs_expr(expr: &Expr, counts: &mut HashMap<LocalId, usize>) {
    fn bump(counts: &mut HashMap<LocalId, usize>, id: LocalId) {
        *counts.entry(id).or_insert(0) += 1;
    }
    match expr {
        Expr::LocalGet(id) | Expr::Update { id, .. } => bump(counts, *id),
        Expr::LocalSet(id, _) => bump(counts, *id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => bump(counts, *array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => bump(counts, *id),
        Expr::SetAdd { set_id, .. } => bump(counts, *set_id),
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            // Closure params are declarations within the closure scope, not
            // uses of an outer binding — skip them. Captures ARE uses of an
            // outer binding.
            for c in captures {
                bump(counts, *c);
            }
            for c in mutable_captures {
                bump(counts, *c);
            }
            count_local_refs_stmts(body, counts);
        }
        _ => {}
    }
    walk_expr_children(expr, &mut |child| count_local_refs_expr(child, counts));
}
