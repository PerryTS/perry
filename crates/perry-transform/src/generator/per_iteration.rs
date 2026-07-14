//! #6345: keep per-iteration `let`/`const` bindings per-iteration across the
//! async/generator state-machine transform.
//!
//! # The bug
//!
//! `generator::lower` hoists EVERY `Stmt::Let` in the function body into one
//! function-activation-wide frame: `collect_hoisted_vars` gathers them,
//! `Stmt::PreallocateBoxes` allocates one box per id in the entry block (once
//! per call), and `rewrite_hoisted_lets_in_stmts` demotes each `Stmt::Let` to
//! a bare `Expr(LocalSet(id, init))`.
//!
//! That is *required* for any binding whose live range crosses a suspend
//! point: the step closure returns at every `await`/`yield` and is re-entered
//! later, so its plain locals (allocas) do not survive — only the boxes it
//! captured do.
//!
//! But it is *wrong* for a `let`/`const` declared inside a loop. JS gives each
//! iteration a FRESH binding, and a closure created in iteration `k` captures
//! iteration `k`'s binding. Collapsing all iterations onto a single box makes
//! every closure observe the LAST value:
//!
//! ```ignore
//! async function main() {
//!   const fns = [];
//!   for (let i = 0; i < 9; i++) {
//!     const j = i;
//!     fns.push(() => console.log(j));   // node: 0..8   perry (pre-fix): 8 x9
//!   }
//!   fns.forEach(f => f());
//!   await 0;                            // <- the await is the only trigger
//! }
//! ```
//!
//! # Why the non-async path is correct
//!
//! It simply leaves the `Stmt::Let` inside the loop body. Codegen therefore
//! re-executes the declaration on every iteration — re-allocating its box when
//! the binding needs one (`boxed_vars.rs`) — and for a binding nothing ever
//! writes, `boxed_vars` declines to box it at all, so the closure
//! snapshot-captures the value at creation time. Both routes yield a distinct
//! per-iteration binding. The async path loses this the moment the `Let` is
//! hoisted out of the loop.
//!
//! # The fix
//!
//! Hoist only what actually has to be hoisted. This pass returns the ids whose
//! `Stmt::Let` must be LEFT IN PLACE inside the loop; `generator::lower`
//! subtracts them from the hoisted set, so they keep their declaration, keep
//! their per-iteration box/snapshot, and never reach `PreallocateBoxes`.
//!
//! An id qualifies only when all of these hold:
//!
//! 1. It is declared by a `Stmt::Let` inside a loop (`for` / `while` /
//!    `do-while`; `for-of`/`for-in` desugar to `Stmt::For` before we run).
//! 2. It is genuinely block-scoped, NOT a `var`. Two independent guards:
//!    HIR emits a `var` as a function-top pre-declaration `Let` *plus* an
//!    in-place `Let` sharing the same id, so `decl_sites > 1` means `var`;
//!    and a `var` read outside the loop shows up as `total_refs >
//!    refs_inside_the_loop` (the #2308 test). A `var` must stay hoisted — one
//!    function-scoped binding, closures see the last value, which is what node
//!    does and what perry already does correctly.
//! 3. Its live range does not cross a suspend point, so it can never be read
//!    after the step closure has been re-entered:
//!    - a loop-body binding qualifies when no use of it appears at or after a
//!      `yield` that follows its declaration (so `const data = await read(f);
//!      cbs.push(() => data)` — the canonical async loop — still qualifies,
//!      because the `await` precedes the declaration);
//!    - a `for`-init binding is loop-carried (the condition and update re-read
//!      it after the body), so it qualifies only when the loop contains no
//!      suspend at all.
//!
//! Anything we cannot prove safe keeps today's hoisting. The conservative
//! direction is to hoist: un-hoisting a binding that IS live across a suspend
//! would lose its value on resume, which is a worse bug than the one being
//! fixed.

use super::hoist_yields::expr_contains_yield;
use crate::unroll::escape_analysis::{
    count_local_refs_expr, count_local_refs_stmt, count_local_refs_stmts,
};
use perry_hir::walker::walk_expr_children_mut;
use perry_hir::{Expr, Stmt};
use perry_types::{LocalId, Type};
use std::collections::{HashMap, HashSet};

/// Ids whose `Stmt::Let` the state-machine transform must leave in place so
/// each loop iteration gets a fresh binding. See the module docs.
pub(crate) fn collect_per_iteration_ids(body: &[Stmt]) -> HashSet<LocalId> {
    let mut total: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmts(body, &mut total);

    // `var` detection: HIR pre-declares a `var` at function top AND re-declares
    // it in place, so its id owns two `Let` sites. A block-scoped `let`/`const`
    // owns exactly one.
    let mut decl_sites: HashMap<LocalId, usize> = HashMap::new();
    count_decl_sites(body, &mut decl_sites);

    let mut out = HashSet::new();
    scan_stmts(body, &total, &decl_sites, &mut out);
    out
}

/// Walk every statement list looking for loops. Each loop is analyzed on its
/// own; nested loops are reached by the recursion and analyzed against the
/// same whole-body reference counts.
fn scan_stmts(
    stmts: &[Stmt],
    total: &HashMap<LocalId, usize>,
    decl_sites: &HashMap<LocalId, usize>,
    out: &mut HashSet<LocalId>,
) {
    for s in stmts {
        if let Some(l) = as_loop(s) {
            analyze_loop(l, total, decl_sites, out);
        }
        each_child_stmt_list(s, &mut |list| scan_stmts(list, total, decl_sites, out));
    }
}

fn analyze_loop(
    loop_stmt: &Stmt,
    total: &HashMap<LocalId, usize>,
    decl_sites: &HashMap<LocalId, usize>,
    out: &mut HashSet<LocalId>,
) {
    // Reference counts confined to this loop (init + condition + update + body).
    // An id whose every use is inside the loop cannot be a `var` that leaks out.
    let mut inside: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmt(loop_stmt, &mut inside);

    let suspends = loop_contains_suspend(loop_stmt);
    let block_scoped = |id: LocalId| -> bool {
        decl_sites.get(&id).copied().unwrap_or(0) == 1
            && total.get(&id).copied().unwrap_or(0) == inside.get(&id).copied().unwrap_or(0)
    };

    // `for (let i = 0; …; i++)` — the binding is loop-carried: the condition and
    // the update re-read it *after* the body has run, so if the body suspends,
    // `i` is live across that suspend and has to stay boxed. With no suspend
    // anywhere in the loop the whole loop runs inside a single state and `i` can
    // keep its per-iteration declaration.
    if !suspends {
        if let Stmt::For {
            init: Some(init), ..
        } = loop_stmt
        {
            if let Stmt::Let {
                id,
                init: init_expr,
                ..
            } = init.as_ref()
            {
                if !init_expr.as_ref().is_some_and(expr_contains_yield) && block_scoped(*id) {
                    out.insert(*id);
                }
            }
        }
    }

    classify_block(loop_body(loop_stmt), &block_scoped, out);
}

/// Classify the `Stmt::Let`s of one block inside a loop.
///
/// Descends through non-loop nesting (`if` / `try` / `switch` / labeled) —
/// `for-in` in particular parks its key binding in an `if` arm inside the
/// desugared `for` body — but stops at nested loops, which `scan_stmts`
/// analyzes in their own right.
fn classify_block(
    block: &[Stmt],
    block_scoped: &dyn Fn(LocalId) -> bool,
    out: &mut HashSet<LocalId>,
) {
    for (i, stmt) in block.iter().enumerate() {
        if let Stmt::Let { id, init, .. } = stmt {
            // A `let __tmp = yield …;` IS the state split: the linearizer
            // assigns it in the resumed state, so it must stay a boxed
            // cross-state local.
            let splits_state = init.as_ref().is_some_and(expr_contains_yield);
            if !splits_state && block_scoped(*id) && !used_after_suspend(*id, &block[i + 1..]) {
                out.insert(*id);
            }
        }
        if !is_loop(stmt) {
            each_child_stmt_list(stmt, &mut |list| classify_block(list, block_scoped, out));
        }
    }
}

/// Is `id` read at or after a suspend point in `rest` (the statements that
/// follow its declaration in the same block)?
///
/// A statement that contains BOTH a yield and a use of `id` is treated as a
/// use-after-suspend: we cannot order the two without a finer analysis, and
/// guessing wrong would drop the binding on resume.
fn used_after_suspend(id: LocalId, rest: &[Stmt]) -> bool {
    let mut suspended = false;
    for stmt in rest {
        let mut refs: HashMap<LocalId, usize> = HashMap::new();
        count_local_refs_stmt(stmt, &mut refs);
        let uses = refs.get(&id).copied().unwrap_or(0) > 0;
        let yields = stmt_contains_suspend(stmt);
        if uses && (suspended || yields) {
            return true;
        }
        suspended |= yields;
    }
    false
}

/// The loop `stmt` is, looking through any `Stmt::Labeled` wrappers.
///
/// `outer: for (…) { … continue outer; … }` lowers to `Labeled { body: For }`,
/// so a bare `matches!(stmt, Stmt::For { .. })` test silently skips every
/// labeled loop — and `each_child_stmt_list` unwraps `Labeled` straight to the
/// inner loop's BODY, so the loop statement itself would never be analyzed at
/// all. Its per-iteration bindings would then keep collapsing onto one box.
fn as_loop(stmt: &Stmt) -> Option<&Stmt> {
    match stmt {
        Stmt::For { .. } | Stmt::While { .. } | Stmt::DoWhile { .. } => Some(stmt),
        Stmt::Labeled { body, .. } => as_loop(body),
        _ => None,
    }
}

fn is_loop(stmt: &Stmt) -> bool {
    as_loop(stmt).is_some()
}

fn loop_body(stmt: &Stmt) -> &[Stmt] {
    match stmt {
        Stmt::For { body, .. } | Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => body,
        _ => &[],
    }
}

fn loop_contains_suspend(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_some_and(|i| stmt_contains_suspend(i))
                || condition.as_ref().is_some_and(expr_contains_yield)
                || update.as_ref().is_some_and(expr_contains_yield)
                || body.iter().any(stmt_contains_suspend)
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_contains_yield(condition) || body.iter().any(stmt_contains_suspend)
        }
        _ => false,
    }
}

/// Conservative "does this statement suspend?" — scans every expression it
/// owns (not just the normalized `yield` statement shapes `body_contains_yield`
/// recognizes) and recurses through all nested blocks, including nested loops.
/// `expr_contains_yield` stops at `Expr::Closure`, so a nested async arrow's
/// own `await`s correctly do not count as a suspend of THIS function.
fn stmt_contains_suspend(stmt: &Stmt) -> bool {
    let mut found = false;
    each_expr(stmt, &mut |e| {
        if !found && expr_contains_yield(e) {
            found = true;
        }
    });
    if found {
        return true;
    }
    let mut nested = false;
    each_child_stmt_list(stmt, &mut |list| {
        if !nested && list.iter().any(stmt_contains_suspend) {
            nested = true;
        }
    });
    nested
}

/// Invoke `f` on each expression directly owned by `stmt` (not those inside
/// nested statement lists — `each_child_stmt_list` covers those).
fn each_expr<F: FnMut(&Expr)>(stmt: &Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                f(e);
            }
        }
        Stmt::If { condition, .. } => f(condition),
        Stmt::While { condition, .. } | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            init,
            condition,
            update,
            ..
        } => {
            if let Some(i) = init {
                each_expr(i, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(t) = &c.test {
                    f(t);
                }
            }
        }
        Stmt::Labeled { body, .. } => each_expr(body, f),
        _ => {}
    }
}

/// Invoke `f` on each nested `&[Stmt]` directly owned by `stmt`.
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
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => f(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                each_child_stmt_list(i, f);
            }
            f(body);
        }
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

// ---------------------------------------------------------------------------
// #6345 part 2: per-iteration bindings that ARE live across a suspend
// ---------------------------------------------------------------------------
//
// `collect_per_iteration_ids` can only un-hoist a binding whose live range
// stays inside one state. When the binding really is read after an `await`,
// it has to keep its cross-state box — and that box is shared by every
// iteration, so a closure capturing it still sees the last value:
//
// ```ignore
// for (let i = 0; i < 9; i++) { await step(); tasks.push(() => i); }
// for (let i = 0; i < 9; i++) { const j = i; await step(); fns.push(() => j); }
// ```
//
// The step closure is re-entered on every resume and its capture slots are
// fixed for its lifetime, so the box pointer itself cannot be swapped per
// iteration — the only per-activation storage that survives a suspend IS that
// box. What we can do instead is give the CLOSURE the value rather than the
// cell: at the moment the closure is built, the shared box holds the current
// iteration's value, so copying it into a fresh local declared immediately
// before the closure — a local that lives and dies inside a single state —
// hands each closure its own binding.
//
// This is only sound when nothing may write the binding after the closure
// captures it, and HIR already answers that question: a captured id lands in
// `mutable_captures` when it is assigned anywhere (by a closure OR by the
// enclosing scope — verified for both), and stays in plain `captures` when it
// is only read. So we snapshot exactly `captures \ mutable_captures`. A
// `for`-init counter is deliberately kept out of `mutable_captures` by HIR
// despite `i++`, which is precisely the per-iteration semantics we want.
// Bindings a closure writes (`let acc = 0; const add = () => acc += 5`) keep
// their shared box and are untouched.

/// Copy per-iteration bindings that outlive a suspend into a fresh local at
/// each closure-creation site. Returns the ids of the locals it introduced;
/// they must be kept out of the hoisted set (they are per-state by design).
///
/// Runs BEFORE linearization so the inserted `Let`s land in the same state as
/// the closure that reads them.
pub(crate) fn snapshot_suspended_loop_captures(
    body: &mut Vec<Stmt>,
    next_local_id: &mut LocalId,
    per_iteration: &HashSet<LocalId>,
) -> HashSet<LocalId> {
    let mut total: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmts(body, &mut total);
    let mut decl_sites: HashMap<LocalId, usize> = HashMap::new();
    count_decl_sites(body, &mut decl_sites);

    let ctx = SnapshotCtx {
        total,
        decl_sites,
        per_iteration: per_iteration.clone(),
    };
    let mut introduced = HashSet::new();
    walk_snapshot(body, &HashSet::new(), &ctx, next_local_id, &mut introduced);
    introduced
}

struct SnapshotCtx {
    total: HashMap<LocalId, usize>,
    decl_sites: HashMap<LocalId, usize>,
    per_iteration: HashSet<LocalId>,
}

/// Per-iteration bindings of `loop_stmt` that are STILL hoisted — i.e. block
/// scoped, declared by this loop, and not already un-hoisted by
/// `collect_per_iteration_ids` (those need no snapshot; they keep a real
/// per-iteration declaration, which also preserves write-sharing).
fn hoisted_loop_bindings(loop_stmt: &Stmt, ctx: &SnapshotCtx) -> HashSet<LocalId> {
    let mut inside: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmt(loop_stmt, &mut inside);

    let mut declared: HashSet<LocalId> = HashSet::new();
    if let Stmt::For {
        init: Some(init), ..
    } = loop_stmt
    {
        if let Stmt::Let { id, .. } = init.as_ref() {
            declared.insert(*id);
        }
    }
    collect_block_lets(loop_body(loop_stmt), &mut declared);

    declared
        .into_iter()
        .filter(|id| {
            !ctx.per_iteration.contains(id)
                && ctx.decl_sites.get(id).copied().unwrap_or(0) == 1
                && ctx.total.get(id).copied().unwrap_or(0) == inside.get(id).copied().unwrap_or(0)
        })
        .collect()
}

/// `Stmt::Let` ids in this block and its non-loop nesting. Nested loops own
/// their own bindings and are handled when the walk reaches them.
fn collect_block_lets(block: &[Stmt], out: &mut HashSet<LocalId>) {
    for s in block {
        if let Stmt::Let { id, .. } = s {
            out.insert(*id);
        }
        if !is_loop(s) {
            each_child_stmt_list(s, &mut |list| collect_block_lets(list, out));
        }
    }
}

fn walk_snapshot(
    stmts: &mut Vec<Stmt>,
    active: &HashSet<LocalId>,
    ctx: &SnapshotCtx,
    next_local_id: &mut LocalId,
    introduced: &mut HashSet<LocalId>,
) {
    // Descend first: a suspending loop adds its still-hoisted per-iteration
    // bindings to the set visible to closures built anywhere inside it.
    for stmt in stmts.iter_mut() {
        // `as_loop` looks through `Stmt::Labeled` — `outer: for (…)` is a
        // labeled wrapper around the loop, and its bindings are per-iteration
        // just the same.
        let nested = match as_loop(stmt) {
            Some(l) if loop_contains_suspend(l) => {
                let mut a = active.clone();
                a.extend(hoisted_loop_bindings(l, ctx));
                a
            }
            _ => active.clone(),
        };
        each_child_stmt_list_mut(stmt, &mut |list| {
            walk_snapshot(list, &nested, ctx, next_local_id, introduced)
        });
    }

    // Then rewrite the closures created at THIS level, inserting each
    // snapshot `Let` immediately before the statement that builds the closure
    // (same block, no intervening `await` — so the same state).
    let taken: Vec<Stmt> = stmts.drain(..).collect();
    let mut out: Vec<Stmt> = Vec::with_capacity(taken.len());
    for mut stmt in taken {
        let mut snap_map: HashMap<LocalId, LocalId> = HashMap::new();
        each_expr_mut(&mut stmt, &mut |e| {
            snapshot_closures_in_expr(e, active, &mut snap_map, next_local_id)
        });
        let mut pairs: Vec<(LocalId, LocalId)> = snap_map.into_iter().collect();
        pairs.sort();
        for (orig, snap) in pairs {
            introduced.insert(snap);
            out.push(Stmt::Let {
                id: snap,
                name: format!("__periter_{}", snap),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::LocalGet(orig)),
            });
        }
        out.push(stmt);
    }
    *stmts = out;
}

/// Find closures in `e` (not descending into a closure's own body — the whole
/// subtree of a rewritten closure is handled in one go) and redirect their
/// read-only captures of `active` bindings to per-iteration snapshots.
fn snapshot_closures_in_expr(
    e: &mut Expr,
    active: &HashSet<LocalId>,
    snap_map: &mut HashMap<LocalId, LocalId>,
    next_local_id: &mut LocalId,
) {
    if let Expr::Closure {
        captures,
        mutable_captures,
        ..
    } = e
    {
        let targets: Vec<LocalId> = captures
            .iter()
            .copied()
            .filter(|id| active.contains(id) && !mutable_captures.contains(id))
            .collect();
        if !targets.is_empty() {
            let mut local_map: HashMap<LocalId, LocalId> = HashMap::new();
            for orig in &targets {
                let snap = *snap_map.entry(*orig).or_insert_with(|| {
                    let id = *next_local_id;
                    *next_local_id = next_local_id.saturating_add(1);
                    id
                });
                local_map.insert(*orig, snap);
            }
            // Substitute on a copy, then prove — with the same exhaustive
            // counter the `var` analysis trusts — that no reference to the
            // original id survives anywhere in the closure. If one does, this
            // rewrite would leave the closure reading an id it no longer
            // captures, so roll the whole closure back and keep the (merely
            // stale) shared-box behaviour instead of inventing a new bug.
            let original = e.clone();
            rename_in_expr(e, &local_map);
            let mut leftover: HashMap<LocalId, usize> = HashMap::new();
            count_local_refs_expr(e, &mut leftover);
            if targets
                .iter()
                .any(|orig| leftover.get(orig).copied().unwrap_or(0) > 0)
            {
                *e = original;
                for orig in &targets {
                    snap_map.remove(orig);
                }
            }
        }
        return;
    }
    walk_expr_children_mut(e, &mut |child| {
        snapshot_closures_in_expr(child, active, snap_map, next_local_id)
    });
}

/// Rewrite USE sites of the mapped ids throughout an expression subtree,
/// including capture lists and nested closure bodies. Declaration ids
/// (`Stmt::Let`) are never rewritten: every binding owns a unique LocalId, so
/// a mapped id can only ever appear as a use inside this subtree.
fn rename_in_expr(e: &mut Expr, map: &HashMap<LocalId, LocalId>) {
    let sub = |id: &mut LocalId| {
        if let Some(new) = map.get(id) {
            *id = *new;
        }
    };
    match e {
        Expr::LocalGet(id) | Expr::Update { id, .. } => sub(id),
        Expr::LocalSet(id, _) => sub(id),
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => sub(array_id),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => sub(id),
        Expr::SetAdd { set_id, .. } => sub(set_id),
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            for c in captures.iter_mut() {
                sub(c);
            }
            for c in mutable_captures.iter_mut() {
                sub(c);
            }
            for s in body.iter_mut() {
                rename_in_stmt(s, map);
            }
        }
        _ => {}
    }
    walk_expr_children_mut(e, &mut |child| rename_in_expr(child, map));
}

fn rename_in_stmt(stmt: &mut Stmt, map: &HashMap<LocalId, LocalId>) {
    each_expr_mut(stmt, &mut |e| rename_in_expr(e, map));
    each_child_stmt_list_mut(stmt, &mut |list| {
        for s in list.iter_mut() {
            rename_in_stmt(s, map);
        }
    });
}

fn each_expr_mut<F: FnMut(&mut Expr)>(stmt: &mut Stmt, f: &mut F) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                f(e);
            }
        }
        Stmt::If { condition, .. } => f(condition),
        Stmt::While { condition, .. } | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            init,
            condition,
            update,
            ..
        } => {
            if let Some(i) = init {
                each_expr_mut(i, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            f(discriminant);
            for c in cases {
                if let Some(t) = &mut c.test {
                    f(t);
                }
            }
        }
        Stmt::Labeled { body, .. } => each_expr_mut(body, f),
        _ => {}
    }
}

fn each_child_stmt_list_mut<F: FnMut(&mut Vec<Stmt>)>(stmt: &mut Stmt, f: &mut F) {
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
                f(&mut c.body);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            f(body);
            if let Some(c) = catch {
                f(&mut c.body);
            }
            if let Some(fin) = finally {
                f(fin);
            }
        }
        Stmt::Labeled { body, .. } => each_child_stmt_list_mut(body, f),
        _ => {}
    }
}

/// Count `Stmt::Let` DECLARATION sites per id (a `var` owns two: the
/// function-top pre-declaration and the in-place one). Does not descend into
/// closure bodies — a closure's locals carry their own distinct ids.
fn count_decl_sites(stmts: &[Stmt], out: &mut HashMap<LocalId, usize>) {
    for s in stmts {
        if let Stmt::Let { id, .. } = s {
            *out.entry(*id).or_insert(0) += 1;
        }
        // A `for (let i = …; …)` init is a `Stmt::Let` that `each_child_stmt_list`
        // does not surface (it owns no statement LIST). Count it explicitly, or
        // every for-init binding would look like it has zero declaration sites
        // and be rejected by the `decl_sites == 1` block-scoped test.
        if let Stmt::For { init: Some(i), .. } = s {
            if let Stmt::Let { id, .. } = i.as_ref() {
                *out.entry(*id).or_insert(0) += 1;
            }
        }
        each_child_stmt_list(s, &mut |list| count_decl_sites(list, out));
    }
}
