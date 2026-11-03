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

use crate::unroll::escape_analysis::{
    count_local_refs_expr, count_local_refs_stmt, count_local_refs_stmts,
};
use perry_hir::types::{LocalId, Type};
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{BinaryOp, Expr, Stmt, UpdateOp, WithSetFallback};
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
                if !init_expr.as_ref().is_some_and(expr_contains_suspend) && block_scoped(*id) {
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
            let splits_state = init.as_ref().is_some_and(expr_contains_suspend);
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
                || condition.as_ref().is_some_and(expr_contains_suspend)
                || update.as_ref().is_some_and(expr_contains_suspend)
                || body.iter().any(stmt_contains_suspend)
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_contains_suspend(condition) || body.iter().any(stmt_contains_suspend)
        }
        _ => false,
    }
}

/// Conservative "does this statement suspend?" — scans every expression it
/// owns (not just the normalized `yield` statement shapes `body_contains_yield`
/// recognizes) and recurses through all nested blocks, including nested loops.
/// `expr_contains_suspend` stops at `Expr::Closure`, so a nested async arrow's
/// own `await`s correctly do not count as a suspend of THIS function.
fn stmt_contains_suspend(stmt: &Stmt) -> bool {
    let mut found = false;
    each_expr(stmt, &mut |e| {
        if !found && expr_contains_suspend(e) {
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

fn expr_contains_suspend(expr: &Expr) -> bool {
    if matches!(expr, Expr::Yield { .. } | Expr::Await(_)) {
        return true;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| {
        if !found && expr_contains_suspend(child) {
            found = true;
        }
    });
    found
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
        Expr::LocalGet(id) | Expr::Update { id, .. } | Expr::ScopedTemp { id, .. } => sub(id),
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
    // #8208: `each_expr_mut` only reaches LocalIds that live inside an Expr, so
    // `ReleaseBoxes` — a bare LocalId list with no sub-expression — used to be
    // walked straight past. That is not cosmetic: a stale id here would release
    // a still-live local's cell and hand it to the next allocation. Rename it
    // like a `LocalSet` target, which is what `Stmt::ReleaseBoxes`' doc
    // requires of any renaming pass.
    //
    // Scoped to `ReleaseBoxes`: the two `Prealloc*` variants carry the same
    // shape and are also unreached here, but that gap is pre-existing and
    // benign, and closing it would change codegen for existing programs.
    if let Stmt::ReleaseBoxes(ids) = stmt {
        for id in ids.iter_mut() {
            if let Some(new_id) = map.get(id) {
                *id = *new_id;
            }
        }
    }
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

// ---------------------------------------------------------------------------
// #6354: per-iteration bindings a closure WRITES that also outlive a suspend
// ---------------------------------------------------------------------------
//
// `snapshot_suspended_loop_captures` (part 2 above) can only hand a closure a
// per-iteration binding when nobody writes it after capture — it copies the
// *value*, and a value snapshot would silently drop any later write. It is
// therefore gated to `captures \ mutable_captures`. A binding a closure assigns
// (`let acc = i; const bump = () => { acc += 100; }`) stays in `mutable_captures`,
// keeps its single activation-wide box, and every closure created in the loop
// then observes the LAST iteration's value:
//
// ```ignore
// for (let i = 0; i < 9; i++) {
//   let acc = i;
//   const bump = () => { acc += 100; };  // WRITES acc
//   bump();
//   await tick();                        // acc read after the suspend
//   fns.push(() => acc);                 // node: 100..108   perry: 108 x9
// }
// ```
//
// The fix reduces this WRITE case to the already-solved READ-ONLY case by
// backing the binding with a one-element heap cell. The binding VARIABLE then
// holds a per-iteration array *reference* that is never reassigned (only its
// element is), so it lands in `captures \ mutable_captures` and part 2 snapshots
// the reference per iteration; writes go to the shared element and stay visible
// to every closure of the same iteration. Concretely `acc` above becomes:
//
// ```ignore
// let acc = [i];
// const bump = () => { acc[0] += 100; };
// bump();
// await tick();
// fns.push(() => acc[0]);
// ```
//
// which perry already compiles correctly. The rewrite runs BEFORE part 1/part 2
// so those passes see the cell form.

/// Loop-body block-scoped `let` bindings that (a) some closure in the loop lists
/// in its `mutable_captures` and (b) are live across a suspend inside the loop.
/// These are exactly the bindings #6345 leaves collapsed: read-only captures are
/// snapshotted, and bindings whose live range stays in one state keep their
/// per-iteration declaration, so neither path covers a written binding that also
/// outlives an `await`. Each returned id should be rewritten to a heap cell by
/// [`rewrite_written_captures_to_cells`].
pub(crate) fn collect_written_suspended_loop_captures(body: &[Stmt]) -> HashSet<LocalId> {
    // The union of every closure's `mutable_captures` (including nested
    // closures) — HIR marks an id here iff it is assigned somewhere it is
    // captured, whether by a closure or by the enclosing scope after capture.
    let mut mutably_captured: HashSet<LocalId> = HashSet::new();
    collect_mutable_captures_stmts(body, &mut mutably_captured);
    if mutably_captured.is_empty() {
        return HashSet::new();
    }

    let mut total: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmts(body, &mut total);
    let mut decl_sites: HashMap<LocalId, usize> = HashMap::new();
    count_decl_sites(body, &mut decl_sites);

    let mut out = HashSet::new();
    scan_written_loops(body, &total, &decl_sites, &mutably_captured, &mut out);

    // Exclude anything referenced in a form the cell rewrite cannot express (a
    // bare-`LocalId` array/set mutation intrinsic, or a `with` fallback). Such a
    // binding is left as-is — the pre-existing collapse persists for that rare
    // shape, but no new corruption is introduced.
    if !out.is_empty() {
        let mut unsafe_ids: HashSet<LocalId> = HashSet::new();
        collect_cell_unsafe_ids(body, &mut unsafe_ids);
        out.retain(|id| !unsafe_ids.contains(id));
    }
    out
}

/// Ids that appear in a reference form [`rewrite_written_captures_to_cells`]
/// does NOT rewrite: array/set mutation intrinsics keyed on a bare `LocalId`
/// (`arr.push(x)` → `ArrayPush { array_id, ..  }`, `set.add(x)`, `arr.pop()`, …) and
/// `with`-statement fallbacks. Turning such a binding into a cell would leave
/// the intrinsic pointing at the one-element cell array instead of the value it
/// holds, so these ids are excluded from the candidate set.
fn collect_cell_unsafe_ids(stmts: &[Stmt], out: &mut HashSet<LocalId>) {
    for s in stmts {
        each_expr(s, &mut |e| collect_cell_unsafe_ids_expr(e, out));
        each_child_stmt_list(s, &mut |list| collect_cell_unsafe_ids(list, out));
    }
}

fn collect_cell_unsafe_ids_expr(e: &Expr, out: &mut HashSet<LocalId>) {
    match e {
        Expr::ArrayPush { array_id, .. }
        | Expr::ArrayPushSpread { array_id, .. }
        | Expr::ArrayUnshift { array_id, .. }
        | Expr::ArraySplice { array_id, .. }
        | Expr::ArrayCopyWithin { array_id, .. } => {
            out.insert(*array_id);
        }
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => {
            out.insert(*id);
        }
        Expr::SetAdd { set_id, .. } => {
            out.insert(*set_id);
        }
        Expr::WithSet { fallback, .. } => {
            if let WithSetFallback::Local(id) | WithSetFallback::SloppyImplicit(id) = fallback {
                out.insert(*id);
            }
        }
        // A closure body can mutate a captured binding the same way — descend.
        Expr::Closure { body, .. } => collect_cell_unsafe_ids(body, out),
        _ => {}
    }
    walk_expr_children(e, &mut |child| collect_cell_unsafe_ids_expr(child, out));
}

fn scan_written_loops(
    stmts: &[Stmt],
    total: &HashMap<LocalId, usize>,
    decl_sites: &HashMap<LocalId, usize>,
    mutably_captured: &HashSet<LocalId>,
    out: &mut HashSet<LocalId>,
) {
    for s in stmts {
        if let Some(l) = as_loop(s) {
            analyze_written_loop(l, total, decl_sites, mutably_captured, out);
        }
        each_child_stmt_list(s, &mut |list| {
            scan_written_loops(list, total, decl_sites, mutably_captured, out)
        });
    }
}

fn analyze_written_loop(
    loop_stmt: &Stmt,
    total: &HashMap<LocalId, usize>,
    decl_sites: &HashMap<LocalId, usize>,
    mutably_captured: &HashSet<LocalId>,
    out: &mut HashSet<LocalId>,
) {
    let mut inside: HashMap<LocalId, usize> = HashMap::new();
    count_local_refs_stmt(loop_stmt, &mut inside);
    let block_scoped = |id: LocalId| -> bool {
        decl_sites.get(&id).copied().unwrap_or(0) == 1
            && total.get(&id).copied().unwrap_or(0) == inside.get(&id).copied().unwrap_or(0)
    };
    classify_written_block(loop_body(loop_stmt), &block_scoped, mutably_captured, out);
}

/// Mirror of `classify_block`, but selecting the bindings part 2 CANNOT fix: a
/// block-scoped loop-body `let` that some closure writes (`mutably_captured`)
/// and that is read at or after a suspend following its declaration. Descends
/// through non-loop nesting; nested loops are reached by `scan_written_loops`.
fn classify_written_block(
    block: &[Stmt],
    block_scoped: &dyn Fn(LocalId) -> bool,
    mutably_captured: &HashSet<LocalId>,
    out: &mut HashSet<LocalId>,
) {
    for (i, stmt) in block.iter().enumerate() {
        if let Stmt::Let { id, init, .. } = stmt {
            let splits_state = init.as_ref().is_some_and(expr_contains_suspend);
            if !splits_state
                && mutably_captured.contains(id)
                && block_scoped(*id)
                && used_after_suspend(*id, &block[i + 1..])
            {
                out.insert(*id);
            }
        }
        if !is_loop(stmt) {
            each_child_stmt_list(stmt, &mut |list| {
                classify_written_block(list, block_scoped, mutably_captured, out)
            });
        }
    }
}

/// Union every closure's `mutable_captures` in these statements, descending into
/// nested closure bodies (a deeper closure's write still makes the binding
/// "assigned after capture").
fn collect_mutable_captures_stmts(stmts: &[Stmt], out: &mut HashSet<LocalId>) {
    for s in stmts {
        each_expr(s, &mut |e| collect_mutable_captures_expr(e, out));
        each_child_stmt_list(s, &mut |list| collect_mutable_captures_stmts(list, out));
    }
}

fn collect_mutable_captures_expr(e: &Expr, out: &mut HashSet<LocalId>) {
    if let Expr::Closure {
        mutable_captures,
        body,
        ..
    } = e
    {
        out.extend(mutable_captures.iter().copied());
        collect_mutable_captures_stmts(body, out);
    }
    walk_expr_children(e, &mut |child| collect_mutable_captures_expr(child, out));
}

/// Rewrite each id in `cells` from a scalar binding into a one-element heap
/// cell: its declaration becomes `let c = [init]`, every read/write/update of
/// `c` becomes an indexed access on `c[0]`, and every closure that captured `c`
/// keeps it as a now read-only capture (dropped from `mutable_captures`). See
/// the module docs — after this, `c`'s VALUE (the array reference) is never
/// reassigned, so part 2 hands each iteration its own cell.
pub(crate) fn rewrite_written_captures_to_cells(body: &mut [Stmt], cells: &HashSet<LocalId>) {
    if cells.is_empty() {
        return;
    }
    let mut hoisted = HoistedCells::default();
    for s in body.iter_mut() {
        each_child_stmt_list_mut(s, &mut |list| {
            hoist_forward_cell_inits(list, cells, &mut hoisted)
        });
    }
    for s in body.iter_mut() {
        rewrite_cells_in_stmt(s, cells, &hoisted);
    }
}

/// The cells whose box was preallocated before their declaration (see
/// [`hoist_forward_cell_inits`]).
#[derive(Default)]
struct HoistedCells {
    /// The cell exists from the prealloc site; its declaration, left where it
    /// is, is the cell's first store rather than the creation of a new cell.
    declared: HashSet<LocalId>,
    /// The cells that stand in for a TDZ box (`Stmt::PreallocateTdzBoxes`),
    /// by id, with the binding's name for the ReferenceError. Such a cell is
    /// `[value, initialized]`: it is born `[undefined, false]`, and only the
    /// declaration's store sets the flag. Every access of the program's own
    /// (read, write, update) is checked against it: `initialized ? access :
    /// throw`. Perry's internal reads of the binding (class-capture
    /// snapshots, the first-evaluation guard) take `cell[0]` unchecked, as the
    /// box they replace is read with the TDZ check suppressed. A cell that
    /// replaces a plain box (`PreallocateBoxes`) or a declaration with no
    /// earlier capture has no dead zone to keep and stays `[value]`.
    checked: HashMap<LocalId, String>,
}

/// The declaration of `id` among `stmts` or the statement lists nested in
/// them (in program order; a declaration is unique to its id): give it an
/// initializer (`let c;` initializes to `undefined`, which
/// [`rewrite_cells_in_stmt`] turns into the cell's first store) and return its
/// name, type and mutability for the cell created at the prealloc site.
fn declare_forward_cell(stmts: &mut [Stmt], id: LocalId) -> Option<(String, Type, bool)> {
    for stmt in stmts {
        if let Stmt::Let {
            id: declared,
            name,
            ty,
            mutable,
            init,
        } = stmt
        {
            if *declared == id {
                init.get_or_insert(Expr::Undefined);
                return Some((name.clone(), ty.clone(), *mutable));
            }
            continue;
        }
        let mut found = None;
        each_child_stmt_list_mut(stmt, &mut |list| {
            if found.is_none() {
                found = declare_forward_cell(list, id);
            }
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A binding that closures capture BEFORE its declaration (a forward-captured
/// `let` / `const` / class, pre-registered by `forward_class_bindings` and
/// friends) has its box allocated by a `PreallocateBoxes` /
/// `PreallocateTdzBoxes` ahead of the first such closure and filled by the
/// declaration. Backed by a cell, the cell must likewise exist before
/// the closure is created (the snapshot pass hands the closure the cell
/// reference), and the declaration must fill THAT cell: a declaration that
/// built a fresh cell would leave every earlier closure with a stale one.
/// So the cell is created where the box was preallocated (`let c = [undefined]`
/// after the rewrite) and the declaration becomes a write to its element: it
/// stays a `Stmt::Let` with an initializer (`let c;` initializes to
/// `undefined`), which [`rewrite_cells_in_stmt`] turns into that write, and
/// which is how it differs from an ordinary assignment of the binding.
/// The declaration is found among the statements after the prealloc or in
/// the statement lists nested in them: a block that wraps the rest of its
/// scope (`using` makes the statements after the resource a `try` body) puts
/// the declaration in a list of its own, while the prealloc stays at the
/// scope's entry. A binding of a `PreallocateTdzBoxes` is recorded in
/// `checked`: its cell keeps the dead zone (see [`HoistedCells`]).
fn hoist_forward_cell_inits(
    stmts: &mut Vec<Stmt>,
    cells: &HashSet<LocalId>,
    hoisted: &mut HoistedCells,
) {
    for s in stmts.iter_mut() {
        each_child_stmt_list_mut(s, &mut |list| {
            hoist_forward_cell_inits(list, cells, hoisted)
        });
    }
    let mut at = 0;
    while at < stmts.len() {
        let forward: Vec<LocalId> = match &stmts[at] {
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => ids
                .iter()
                .copied()
                .filter(|id| cells.contains(id))
                .collect(),
            _ => Vec::new(),
        };
        let in_dead_zone = matches!(&stmts[at], Stmt::PreallocateTdzBoxes(_));
        let mut predeclared: Vec<Stmt> = Vec::new();
        for id in forward {
            let Some((name, ty, mutable)) = declare_forward_cell(&mut stmts[at + 1..], id) else {
                continue;
            };
            hoisted.declared.insert(id);
            if in_dead_zone {
                hoisted.checked.insert(id, name.clone());
            }
            predeclared.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init: None,
            });
            if let Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) = &mut stmts[at] {
                ids.retain(|i| *i != id);
            }
        }
        let emptied = matches!(
            &stmts[at],
            Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) if ids.is_empty()
        );
        let at_next = at + predeclared.len() + usize::from(!emptied);
        let insert_at = at + usize::from(!emptied);
        if emptied {
            stmts.remove(at);
        }
        for (k, decl) in predeclared.into_iter().enumerate() {
            stmts.insert(insert_at + k, decl);
        }
        at = at_next;
    }
}

fn rewrite_cells_in_stmt(stmt: &mut Stmt, cells: &HashSet<LocalId>, hoisted: &HoistedCells) {
    match stmt {
        // The declaration of a cell created earlier, where its box was
        // preallocated: the cell's first store, which also ends its dead zone.
        Stmt::Let {
            id,
            init: init @ Some(_),
            ..
        } if hoisted.declared.contains(id) => {
            let id = *id;
            let mut value = init.take().unwrap_or(Expr::Undefined);
            rewrite_cells_in_expr(&mut value, cells, hoisted);
            let store = Expr::IndexSet {
                object: Box::new(Expr::LocalGet(id)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(value),
                byte_offset: 0,
                byte_end: 0,
            };
            *stmt = Stmt::Expr(if hoisted.checked.contains_key(&id) {
                Expr::Sequence(vec![store, cell_flag_set(id)])
            } else {
                store
            });
            return;
        }
        // The candidate's declaration: wrap its initializer in a one-element
        // array so the slot holds a fresh cell each iteration. `let c;` (no
        // init) seeds `[undefined]`; a dead-zone cell seeds `[undefined, false]`.
        Stmt::Let { id, init, ty, .. } if cells.contains(id) => {
            let mut inner = init.take().unwrap_or(Expr::Undefined);
            rewrite_cells_in_expr(&mut inner, cells, hoisted);
            let mut slots = vec![inner];
            if hoisted.checked.contains_key(id) {
                slots.push(Expr::Bool(false));
            }
            *init = Some(Expr::Array(slots, None));
            *ty = Type::Array(Box::new(Type::Any));
        }
        _ => {
            each_expr_mut(stmt, &mut |e| rewrite_cells_in_expr(e, cells, hoisted));
        }
    }
    each_child_stmt_list_mut(stmt, &mut |list| {
        for s in list.iter_mut() {
            rewrite_cells_in_stmt(s, cells, hoisted);
        }
    });
}

/// `cell[slot]` for the cell bound to `id`.
fn cell_slot(id: LocalId, slot: i64) -> Expr {
    Expr::IndexGet {
        byte_offset: 0,
        byte_end: 0,
        object: Box::new(Expr::LocalGet(id)),
        index: Box::new(Expr::Integer(slot)),
    }
}

/// `cell[1] = true`: the declaration ended the cell's dead zone.
fn cell_flag_set(id: LocalId) -> Expr {
    Expr::IndexSet {
        object: Box::new(Expr::LocalGet(id)),
        index: Box::new(Expr::Integer(1)),
        value: Box::new(Expr::Bool(true)),
        byte_offset: 0,
        byte_end: 0,
    }
}

/// `cell[1] ? on_initialized : throw ReferenceError`: the program's own
/// access of a dead-zone cell, where `on_initialized` is the access itself and
/// `on_dead_zone` runs before the throw.
fn cell_checked(id: LocalId, name: &str, on_initialized: Expr, on_dead_zone: Option<Expr>) -> Expr {
    let throw = perry_hir::tdz_check::throw(name);
    Expr::Conditional {
        condition: Box::new(cell_slot(id, 1)),
        then_expr: Box::new(on_initialized),
        else_expr: Box::new(match on_dead_zone {
            Some(undo) => Expr::Sequence(vec![undo, throw]),
            None => throw,
        }),
    }
}

/// A capture operand Perry itself evaluates (a class-capture snapshot or the
/// capture forwards of `new C()`), not the program: the binding may still be
/// in its dead zone, as the class is declared before it (`class Holder {
/// value() { return C } } class C {}`), and the snapshot then holds
/// `undefined` instead of throwing. Codegen gives the box it replaces the
/// same treatment with a TDZ-suppression window around the operand, which a
/// cell's explicit throw would escape, so the cell's value is read directly.
fn rewrite_capture_operand(e: &mut Expr, cells: &HashSet<LocalId>, hoisted: &HoistedCells) {
    match e {
        Expr::LocalGet(id) if cells.contains(id) => *e = cell_slot(*id, 0),
        _ => rewrite_cells_in_expr(e, cells, hoisted),
    }
}

fn rewrite_cells_in_expr(e: &mut Expr, cells: &HashSet<LocalId>, hoisted: &HoistedCells) {
    match e {
        Expr::LocalGet(id) if cells.contains(id) => {
            let id = *id;
            // A dead-zone cell is read through its flag: the program's own
            // read before the declaration throws, as the box it replaces did.
            *e = match hoisted.checked.get(&id) {
                Some(name) => cell_checked(id, name, cell_slot(id, 0), None),
                None => cell_slot(id, 0),
            };
            // Do NOT descend — the freshly built `LocalGet(id)` is the cell
            // reference itself, not another read of the (now indexed) binding.
            return;
        }
        // The first-evaluation guard is Perry's own read of a class binding:
        // in the dead zone it sees `undefined` (not the first evaluation)
        // instead of throwing, so only the program's own reads are checked.
        Expr::ClassIsFirstEvaluation { value, .. } => {
            if let Expr::LocalGet(id) = value.as_ref() {
                let id = *id;
                if cells.contains(&id) {
                    **value = cell_slot(id, 0);
                    return;
                }
            }
        }
        Expr::RegisterClassCaptures { captures, .. } => {
            for capture in captures.iter_mut() {
                rewrite_capture_operand(capture, cells, hoisted);
            }
            return;
        }
        Expr::RefreshClassExprCaptures {
            class_value,
            captures,
            ..
        } => {
            rewrite_cells_in_expr(class_value, cells, hoisted);
            for capture in captures.iter_mut() {
                rewrite_capture_operand(capture, cells, hoisted);
            }
            return;
        }
        // The operands are rewritten apart from the rest of the node: the
        // walk below would read the rewritten ones as program reads again.
        Expr::ClassExprFresh { captured_args, .. } => {
            let mut operands = std::mem::take(captured_args);
            for capture in operands.iter_mut() {
                rewrite_capture_operand(capture, cells, hoisted);
            }
            walk_expr_children_mut(e, &mut |child| rewrite_cells_in_expr(child, cells, hoisted));
            if let Expr::ClassExprFresh { captured_args, .. } = e {
                *captured_args = operands;
            }
            return;
        }
        Expr::New {
            args,
            cap_args_appended,
            ..
        } if *cap_args_appended > 0 => {
            let forwarded = args.len().saturating_sub(*cap_args_appended as usize);
            let mut operands = args.split_off(forwarded);
            for capture in operands.iter_mut() {
                rewrite_capture_operand(capture, cells, hoisted);
            }
            walk_expr_children_mut(e, &mut |child| rewrite_cells_in_expr(child, cells, hoisted));
            if let Expr::New { args, .. } = e {
                args.extend(operands);
            }
            return;
        }
        Expr::LocalSet(id, value) if cells.contains(id) => {
            let id = *id;
            // Rewrite reads of other cells (and of this cell) inside the value
            // BEFORE lifting it out, e.g. `c = c + 1` → `c[0] = c[0] + 1`.
            rewrite_cells_in_expr(value, cells, hoisted);
            let value = std::mem::replace(value.as_mut(), Expr::Undefined);
            let store = Expr::IndexSet {
                object: Box::new(Expr::LocalGet(id)),
                index: Box::new(Expr::Integer(0)),
                value: Box::new(value),
                byte_offset: 0,
                byte_end: 0,
            };
            // A write of a dead-zone cell is checked after its value is
            // evaluated, as `x = rhs` is: the value is staged in the (never
            // observable) value slot, which the throw puts back to
            // `undefined`. Only the declaration's store, which is not a
            // `LocalSet` any more (see `rewrite_cells_in_stmt`), ends the dead
            // zone. The expression evaluates to the value.
            *e = match hoisted.checked.get(&id) {
                Some(name) => {
                    let unstage = Expr::IndexSet {
                        object: Box::new(Expr::LocalGet(id)),
                        index: Box::new(Expr::Integer(0)),
                        value: Box::new(Expr::Undefined),
                        byte_offset: 0,
                        byte_end: 0,
                    };
                    Expr::Sequence(vec![
                        store,
                        cell_checked(id, name, cell_slot(id, 0), Some(unstage)),
                    ])
                }
                None => store,
            };
            return;
        }
        Expr::Update { id, op, prefix } if cells.contains(id) => {
            let id = *id;
            let op = match op {
                UpdateOp::Increment => BinaryOp::Add,
                UpdateOp::Decrement => BinaryOp::Sub,
            };
            let prefix = *prefix;
            let update = Expr::IndexUpdate {
                object: Box::new(Expr::LocalGet(id)),
                index: Box::new(Expr::Integer(0)),
                op,
                prefix,
                strict: false,
                byte_offset: 0,
                byte_end: 0,
            };
            // `x++` reads `x` first: in the dead zone it throws before it
            // changes anything.
            *e = match hoisted.checked.get(&id) {
                Some(name) => {
                    Expr::Sequence(vec![cell_checked(id, name, Expr::Undefined, None), update])
                }
                None => update,
            };
            return;
        }
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            // The cell is now mutated only through `IndexSet`/`IndexUpdate` on
            // its element — the VARIABLE is never reassigned — so it is a
            // read-only capture. Drop it from `mutable_captures` (keeping it in
            // `captures` so the body can still read the array reference) so part
            // 2's `captures \ mutable_captures` gate snapshots it per iteration.
            let demoted: Vec<LocalId> = mutable_captures
                .iter()
                .copied()
                .filter(|c| cells.contains(c))
                .collect();
            mutable_captures.retain(|c| !cells.contains(c));
            for c in demoted {
                if !captures.contains(&c) {
                    captures.push(c);
                }
            }
            for s in body.iter_mut() {
                rewrite_cells_in_stmt(s, cells, hoisted);
            }
            // Fall through to `walk_expr_children_mut`, which for a closure
            // visits ONLY its param defaults (not the body — handled above) —
            // so a cell referenced in a default (`(x = acc) => …`) is rewritten
            // too. It does not re-descend into the body, so no double-rewrite.
        }
        _ => {}
    }
    walk_expr_children_mut(e, &mut |child| rewrite_cells_in_expr(child, cells, hoisted));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_local_used_after_await_stays_hoisted() {
        let id = 1;
        let body = vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![
                Stmt::Let {
                    id,
                    name: "value".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::String("x".into())),
                },
                Stmt::Expr(Expr::Await(Box::new(Expr::Integer(0)))),
                Stmt::Expr(Expr::LocalGet(id)),
            ],
        }];

        assert!(!collect_per_iteration_ids(&body).contains(&id));
    }

    #[test]
    fn for_loop_local_used_after_await_stays_hoisted() {
        let id = 1;
        let body = vec![Stmt::For {
            init: None,
            condition: Some(Expr::Bool(true)),
            update: None,
            body: vec![
                Stmt::Let {
                    id,
                    name: "value".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::String("x".into())),
                },
                Stmt::Expr(Expr::Await(Box::new(Expr::Integer(0)))),
                Stmt::Expr(Expr::LocalGet(id)),
            ],
        }];

        assert!(!collect_per_iteration_ids(&body).contains(&id));
    }

    const X: LocalId = 2;

    fn read_x() -> Expr {
        Expr::LocalGet(X)
    }

    fn guard_x() -> Expr {
        Expr::ClassIsFirstEvaluation {
            value: Box::new(Expr::LocalGet(X)),
            template: "C".into(),
        }
    }

    /// A loop body that declares `x` after a closure (`closure_body`) uses it:
    /// `prealloc` allocates its box first, and `between` runs after the closure
    /// and before the declaration. Returns the loop's statements after the cell
    /// rewrite.
    fn forward_cell_loop(prealloc: Stmt, closure_body: Vec<Stmt>, between: Vec<Stmt>) -> Vec<Stmt> {
        forward_cell_loop_in(prealloc, closure_body, between, false)
    }

    /// [`forward_cell_loop`], optionally with everything after the closure
    /// wrapped in a `try` the way `using` wraps the rest of its scope: the
    /// declaration is then in a list of its own, nested under the prealloc's.
    fn forward_cell_loop_in(
        prealloc: Stmt,
        closure_body: Vec<Stmt>,
        between: Vec<Stmt>,
        rest_in_try: bool,
    ) -> Vec<Stmt> {
        let (x, read) = (X, 3);
        let closure = Expr::Closure {
            func_id: 900,
            params: Vec::new(),
            return_type: Type::Any,
            body: closure_body,
            captures: vec![x],
            mutable_captures: vec![x],
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: true,
            is_strict: false,
            is_async: false,
            is_generator: false,
        };
        let mut loop_body = vec![
            prealloc,
            Stmt::Let {
                id: read,
                name: "read".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(closure),
            },
        ];
        let mut rest = between;
        rest.extend([
            Stmt::Let {
                id: x,
                name: "x".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(Expr::Integer(1)),
            },
            Stmt::Expr(Expr::Await(Box::new(Expr::Integer(0)))),
        ]);
        if rest_in_try {
            loop_body.push(Stmt::Try {
                body: rest,
                catch: None,
                finally: Some(vec![Stmt::Expr(Expr::Undefined)]),
            });
        } else {
            loop_body.extend(rest);
        }
        let mut body = vec![Stmt::While {
            condition: Expr::Bool(true),
            body: loop_body,
        }];
        rewrite_written_captures_to_cells(&mut body, &HashSet::from([x]));
        let Stmt::While { body, .. } = body.remove(0) else {
            unreachable!("the loop stays a loop");
        };
        body
    }

    fn closure_body(stmts: &[Stmt]) -> &[Stmt] {
        let Some(Stmt::Let {
            init: Some(Expr::Closure { body, .. }),
            ..
        }) = stmts.iter().find(|s| matches!(s, Stmt::Let { id: 3, .. }))
        else {
            panic!("the closure declaration is gone: {stmts:?}");
        };
        body
    }

    fn closure_return(stmts: &[Stmt]) -> &Expr {
        let [Stmt::Return(Some(read))] = closure_body(stmts) else {
            panic!("the closure body changed shape: {:?}", closure_body(stmts));
        };
        read
    }

    fn reads_closure(read: Expr) -> Vec<Stmt> {
        vec![Stmt::Return(Some(read))]
    }

    fn writes_closure(write: Expr) -> Vec<Stmt> {
        vec![Stmt::Expr(write)]
    }

    /// The statements of the loop body that store into slot `slot` of cell `X`.
    fn count_slot_stores(stmts: &[Stmt], slot: i64) -> usize {
        format!("{stmts:?}")
            .matches(&format!(
                "IndexSet {{ object: LocalGet({X}), index: Integer({slot}), value:"
            ))
            .count()
    }

    // A forward-captured binding whose box was allocated in the dead zone
    // keeps its dead zone in the cell: born `[undefined, false]`, set by the
    // declaration, read through the flag.
    #[test]
    fn dead_zone_binding_keeps_its_dead_zone_in_the_cell() {
        let stmts = forward_cell_loop(
            Stmt::PreallocateTdzBoxes(vec![2]),
            reads_closure(read_x()),
            vec![],
        );
        assert!(
            !stmts
                .iter()
                .any(|s| matches!(s, Stmt::PreallocateTdzBoxes(_))),
            "{stmts:?}"
        );
        let Stmt::Let {
            id: 2,
            init: Some(Expr::Array(slots, _)),
            ..
        } = &stmts[0]
        else {
            panic!("the cell is not created where the box was: {stmts:?}");
        };
        assert!(
            matches!(slots.as_slice(), [Expr::Undefined, Expr::Bool(false)]),
            "{slots:?}"
        );
        let Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } = closure_return(&stmts)
        else {
            panic!("the read is not checked: {:?}", closure_return(&stmts));
        };
        assert!(
            matches!(condition.as_ref(), Expr::IndexGet { index, .. } if matches!(index.as_ref(), Expr::Integer(1)))
        );
        assert!(
            matches!(then_expr.as_ref(), Expr::IndexGet { index, .. } if matches!(index.as_ref(), Expr::Integer(0)))
        );
        assert!(format!("{else_expr:?}").contains(perry_hir::tdz_check::TDZ_THROW));
        // the declaration stores the value, then marks the cell initialized
        let decl = stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Expr(Expr::Sequence(parts)) => Some(parts),
                _ => None,
            })
            .expect("the declaration is a store into the cell");
        assert!(
            matches!(
                decl.as_slice(),
                [
                    Expr::IndexSet { .. },
                    Expr::IndexSet { value, .. },
                ] if matches!(value.as_ref(), Expr::Bool(true))
            ),
            "{decl:?}"
        );
    }

    // `using` wraps the statements after its resource in a `try`, which puts a
    // later declaration in a list of its own while the box is preallocated at
    // the scope's entry: the declaration still fills the cell that exists
    // before the closure, rather than building a second one the closure never
    // sees.
    #[test]
    fn declaration_inside_a_try_wrapper_fills_the_cell_created_at_the_prealloc() {
        let stmts = forward_cell_loop_in(
            Stmt::PreallocateTdzBoxes(vec![X]),
            reads_closure(read_x()),
            vec![],
            true,
        );
        assert!(
            !stmts
                .iter()
                .any(|s| matches!(s, Stmt::PreallocateTdzBoxes(_))),
            "{stmts:?}"
        );
        let Stmt::Let {
            id: X,
            init: Some(Expr::Array(slots, _)),
            ..
        } = &stmts[0]
        else {
            panic!("the cell is not created where the box was: {stmts:?}");
        };
        assert!(
            matches!(slots.as_slice(), [Expr::Undefined, Expr::Bool(false)]),
            "{slots:?}"
        );
        let Some(Stmt::Try { body, .. }) = stmts.iter().find(|s| matches!(s, Stmt::Try { .. }))
        else {
            panic!("the wrapper is gone: {stmts:?}");
        };
        assert!(
            !body.iter().any(|s| matches!(s, Stmt::Let { id: X, .. })),
            "the declaration must not build another cell: {body:?}"
        );
        let Some(Stmt::Expr(Expr::Sequence(decl))) = body.first() else {
            panic!("the declaration is not a store into the cell: {body:?}");
        };
        assert!(
            matches!(
                decl.as_slice(),
                [Expr::IndexSet { .. }, Expr::IndexSet { value, .. }]
                    if matches!(value.as_ref(), Expr::Bool(true))
            ),
            "{decl:?}"
        );
        assert!(
            matches!(closure_return(&stmts), Expr::Conditional { .. }),
            "the read stays checked against the dead zone: {:?}",
            closure_return(&stmts)
        );
    }

    // A box that was never in a dead zone has nothing to keep: a one-slot
    // cell and plain reads.
    #[test]
    fn plain_forward_binding_stays_a_one_slot_cell() {
        let stmts = forward_cell_loop(
            Stmt::PreallocateBoxes(vec![2]),
            reads_closure(read_x()),
            vec![],
        );
        let Stmt::Let {
            id: 2,
            init: Some(Expr::Array(slots, _)),
            ..
        } = &stmts[0]
        else {
            panic!("the cell is not created where the box was: {stmts:?}");
        };
        assert!(matches!(slots.as_slice(), [Expr::Undefined]), "{slots:?}");
        assert!(
            matches!(closure_return(&stmts), Expr::IndexGet { .. }),
            "{:?}",
            closure_return(&stmts)
        );
    }

    // The first-evaluation guard is an internal read: in the dead zone it
    // sees `undefined`, it does not throw.
    #[test]
    fn first_evaluation_guard_reads_the_dead_zone_cell_unchecked() {
        let stmts = forward_cell_loop(
            Stmt::PreallocateTdzBoxes(vec![2]),
            reads_closure(guard_x()),
            vec![],
        );
        let Expr::ClassIsFirstEvaluation { value, .. } = closure_return(&stmts) else {
            panic!("{:?}", closure_return(&stmts));
        };
        assert!(
            matches!(value.as_ref(), Expr::IndexGet { index, .. } if matches!(index.as_ref(), Expr::Integer(0))),
            "{value:?}"
        );
    }

    fn is_cell_value_read(e: &Expr) -> bool {
        matches!(
            e,
            Expr::IndexGet { object, index , .. }
                if matches!(object.as_ref(), Expr::LocalGet(X))
                    && matches!(index.as_ref(), Expr::Integer(0))
        )
    }

    // A write of a dead-zone cell evaluates its value, then checks the flag
    // and throws without leaving the value behind; only the declaration's
    // store sets the flag.
    #[test]
    fn dead_zone_write_is_checked_and_only_the_declaration_initializes() {
        let stmts = forward_cell_loop(
            Stmt::PreallocateTdzBoxes(vec![X]),
            writes_closure(Expr::LocalSet(X, Box::new(Expr::Integer(9)))),
            vec![],
        );
        let [Stmt::Expr(Expr::Sequence(write))] = closure_body(&stmts) else {
            panic!(
                "the write is not staged and checked: {:?}",
                closure_body(&stmts)
            );
        };
        let [Expr::IndexSet { value, .. }, Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        }] = write.as_slice()
        else {
            panic!("{write:?}");
        };
        assert!(matches!(value.as_ref(), Expr::Integer(9)), "{value:?}");
        assert!(
            matches!(condition.as_ref(), Expr::IndexGet { index, .. } if matches!(index.as_ref(), Expr::Integer(1)))
        );
        assert!(is_cell_value_read(then_expr), "{then_expr:?}");
        let Expr::Sequence(throwing) = else_expr.as_ref() else {
            panic!("{else_expr:?}");
        };
        assert!(
            matches!(
                throwing.as_slice(),
                [Expr::IndexSet { value, .. }, _] if matches!(value.as_ref(), Expr::Undefined)
            ),
            "the failed write must not leave its value: {throwing:?}"
        );
        assert!(format!("{throwing:?}").contains(perry_hir::tdz_check::TDZ_THROW));
        // the declaration alone raises the flag
        assert_eq!(count_slot_stores(&stmts, 1), 1, "{stmts:?}");
    }

    // `x++` reads `x` first, so it throws in the dead zone before it changes
    // anything.
    #[test]
    fn dead_zone_update_is_checked_before_it_changes_the_cell() {
        let stmts = forward_cell_loop(
            Stmt::PreallocateTdzBoxes(vec![X]),
            writes_closure(Expr::Update {
                id: X,
                op: UpdateOp::Increment,
                prefix: false,
            }),
            vec![],
        );
        let [Stmt::Expr(Expr::Sequence(update))] = closure_body(&stmts) else {
            panic!("the update is not checked: {:?}", closure_body(&stmts));
        };
        let [Expr::Conditional { else_expr, .. }, Expr::IndexUpdate { .. }] = update.as_slice()
        else {
            panic!("{update:?}");
        };
        assert!(format!("{else_expr:?}").contains(perry_hir::tdz_check::TDZ_THROW));
        assert_eq!(count_slot_stores(&stmts, 1), 1, "{stmts:?}");
    }

    // A cell that replaced a plain box has no dead zone: its writes and
    // updates stay bare element accesses.
    #[test]
    fn plain_forward_binding_writes_stay_unchecked() {
        let stmts = forward_cell_loop(
            Stmt::PreallocateBoxes(vec![X]),
            vec![
                Stmt::Expr(Expr::LocalSet(X, Box::new(Expr::Integer(9)))),
                Stmt::Expr(Expr::Update {
                    id: X,
                    op: UpdateOp::Increment,
                    prefix: false,
                }),
            ],
            vec![],
        );
        assert!(
            matches!(
                closure_body(&stmts),
                [
                    Stmt::Expr(Expr::IndexSet { .. }),
                    Stmt::Expr(Expr::IndexUpdate { .. })
                ]
            ),
            "{:?}",
            closure_body(&stmts)
        );
    }

    fn class_new(args: Vec<Expr>, cap_args_appended: u32) -> Expr {
        Expr::New {
            byte_end: 0,
            class_name: "Holder".into(),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
            cap_args_appended,
        }
    }

    // The class-capture snapshots and the capture forwards of `new C()` are
    // Perry's own reads: a class declared before a dead-zone binding it
    // captures snapshots it while it is still `undefined`, so they read the
    // value slot and do not throw. User arguments are the program's reads.
    #[test]
    fn class_capture_operands_read_the_dead_zone_cell_unchecked() {
        let between = vec![
            Stmt::Expr(Expr::RegisterClassCaptures {
                class_name: "Holder".into(),
                captures: vec![read_x()],
            }),
            Stmt::Expr(Expr::RefreshClassExprCaptures {
                class_value: Box::new(Expr::Undefined),
                captures: vec![read_x()],
                env_class: None,
            }),
            Stmt::Expr(Expr::ClassExprFresh {
                template: "Holder".into(),
                evaluation_owner: None,
                named_statics: Vec::new(),
                computed_keys: Vec::new(),
                computed_statics: Vec::new(),
                static_init_order: Vec::new(),
                captured_args: vec![read_x()],
                shared_first_evaluation: None,
                evaluated_parent: None,
                definition_steps: Vec::new(),
            }),
            Stmt::Expr(class_new(vec![read_x(), read_x()], 1)),
        ];
        let stmts = forward_cell_loop(
            Stmt::PreallocateTdzBoxes(vec![X]),
            reads_closure(read_x()),
            between,
        );
        let mut seen = 0;
        for s in &stmts {
            let Stmt::Expr(e) = s else { continue };
            match e {
                Expr::RegisterClassCaptures { captures, .. }
                | Expr::RefreshClassExprCaptures { captures, .. }
                | Expr::ClassExprFresh {
                    captured_args: captures,
                    ..
                } => {
                    assert!(
                        matches!(captures.as_slice(), [c] if is_cell_value_read(c)),
                        "{e:?}"
                    );
                    seen += 1;
                }
                Expr::New { args, .. } => {
                    let [user, forwarded] = args.as_slice() else {
                        panic!("{e:?}");
                    };
                    assert!(
                        matches!(user, Expr::Conditional { .. }),
                        "the program's own argument is a checked read: {user:?}"
                    );
                    assert!(is_cell_value_read(forwarded), "{forwarded:?}");
                    seen += 1;
                }
                _ => {}
            }
        }
        assert_eq!(seen, 4, "{stmts:?}");
    }
}
