//! Boxing analysis: determines which local variables need heap-boxed storage
//! so that closures and the enclosing function can share mutable state.
//!
//! Extracted from `codegen.rs` — purely structural refactor, no logic changes.

use std::collections::{HashMap, HashSet};

use crate::collectors::{collect_let_ids, collect_ref_ids_in_stmts};

/// Determine which local ids in the given statement sequence need
/// heap-boxed storage. An id gets boxed when:
///
/// 1. It's declared in these statements (or a nested block)
/// 2. AND it's captured by at least one closure in the same scope
/// 3. AND it's mutated by someone — either the enclosing function
///    updates/sets it, or at least one closure updates/sets it
///
/// Without boxing, each closure captures a SNAPSHOT of the value at
/// creation time, and multiple closures over the same variable never
/// see each other's mutations. Boxing moves the storage to the heap
/// so all closures (and the enclosing function) share one cell.
///
/// Limitations of this analysis:
/// - We box on capture + any mutation, even if the mutation is in
///   unreachable code. That's safe (just slightly worse perf).
/// - We don't distinguish "mutated inside the closure" from "mutated
///   outside"; both imply boxing. Again, safe.
/// - Params are not boxed here because Stmt::Let handles box
///   allocation; params are handled separately at FnCtx setup time
///   (we don't box them yet — TODO if needed).
pub(crate) fn collect_boxed_vars(
    stmts: &[perry_hir::Stmt],
) -> HashSet<u32> {
    let mut boxed = collect_boxed_vars_scope(stmts);
    // Recurse into nested closures: each inner closure is its own
    // scope and needs independent boxing analysis. Without this,
    // mutable captures inside Promise executors, setTimeout callbacks,
    // etc. never get boxed and the outer mutation is lost.
    collect_nested_closure_boxed_vars_in_stmts(stmts, &mut boxed);
    boxed
}

/// Boxing analysis for a single lexical scope (does NOT recurse into
/// inner closures — that's done by the caller via
/// `collect_nested_closure_boxed_vars_in_stmts`).
fn collect_boxed_vars_scope(stmts: &[perry_hir::Stmt]) -> HashSet<u32> {
    // Step 1: set of ids declared in this scope (Let).
    let mut declared: HashSet<u32> = HashSet::new();
    collect_let_ids(stmts, &mut declared);

    // Step 2: set of ids referenced inside ANY closure body found in
    // these statements. Walk the AST looking for Closure exprs and
    // collect their body refs.
    let mut closure_refs: HashSet<u32> = HashSet::new();
    let mut closure_writes: HashSet<u32> = HashSet::new();
    collect_closure_refs_and_writes_in_stmts(stmts, &mut closure_refs, &mut closure_writes);

    // Step 3: set of ids MUTATED in the enclosing scope (outside any
    // closure). If the enclosing function does `x++` or `x = ...` on
    // a captured var, that var also needs boxing so the closures see
    // the outer updates.
    let mut outer_writes: HashSet<u32> = HashSet::new();
    collect_outer_writes_in_stmts(stmts, &mut outer_writes);

    // Step 4: ids declared in a `for` loop init (the `for (let i = ...;
    // ...; i++)` pattern). These get a fresh binding per iteration
    // under JS spec (`let` scoping), so closures inside the loop body
    // that capture them should each see the iteration's own value.
    // Boxing such a var would make every closure see the LAST iteration's
    // value, which breaks the classic `for (let i=0; i<5; i++) funcs.
    // push(() => i)` pattern.
    //
    // Keep the simpler semantics here: don't box for-init vars. This
    // reverts them to the pre-box snapshot-capture path where each
    // closure stores the current value at creation time, which
    // happens to produce the right result for loop counters.
    let mut for_init_ids: HashSet<u32> = HashSet::new();
    collect_for_init_ids(stmts, &mut for_init_ids);

    // Step 5: detect self-recursive closures. When a Stmt::Let has a
    // Closure init and the closure's body references the Let's own id,
    // that id needs boxing. The initial store (`let fib = closure(...)`)
    // happens AFTER the closure captures are populated, so without
    // boxing the closure would capture 0.0 (the uninitialized slot
    // value). With a box, the closure captures the box POINTER, and
    // the first read of fib from inside the body goes through
    // js_box_get which returns the real closure value.
    let mut self_recursive_ids: HashSet<u32> = HashSet::new();
    collect_self_recursive_closure_ids(stmts, &closure_refs, &mut self_recursive_ids);

    // Box = (declared AND captured AND mutated) OR (self-recursive closure),
    // minus for-loop init vars.
    let mut boxed: HashSet<u32> = HashSet::new();
    for id in &declared {
        if for_init_ids.contains(id) {
            continue;
        }
        if self_recursive_ids.contains(id) {
            boxed.insert(*id);
            continue;
        }
        if closure_refs.contains(id)
            && (closure_writes.contains(id) || outer_writes.contains(id))
        {
            boxed.insert(*id);
        }
    }
    boxed
}

/// Walk the given statements looking for `Expr::Closure` nodes, and
/// for each one recursively run the boxing analysis on its body.
/// Unions the resulting ids into `out`.
///
/// This lets us detect mutable-capture patterns like:
///
/// ```ignore
/// await new Promise((resolve) => {
///     let data = "";                // inner closure's scope
///     rs.on("data", (chunk) => { data += chunk; });
///     rs.on("end", () => resolve(data));
/// });
/// ```
///
/// Without this recursion, `data` is invisible to the top-level
/// `collect_let_ids` walker (which stops at closure boundaries), so
/// the inner closures end up capturing by-value snapshots and the
/// mutation is lost.
fn collect_nested_closure_boxed_vars_in_stmts(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        collect_nested_closure_boxed_vars_in_stmt(s, out);
    }
}

fn collect_nested_closure_boxed_vars_in_stmt(
    stmt: &perry_hir::Stmt,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            collect_nested_closure_boxed_vars_in_expr(e, out);
        }
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_nested_closure_boxed_vars_in_expr(e, out);
            }
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_nested_closure_boxed_vars_in_expr(e, out);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            collect_nested_closure_boxed_vars_in_expr(condition, out);
            collect_nested_closure_boxed_vars_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                collect_nested_closure_boxed_vars_in_stmts(eb, out);
            }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init {
                collect_nested_closure_boxed_vars_in_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_nested_closure_boxed_vars_in_expr(c, out);
            }
            if let Some(u) = update {
                collect_nested_closure_boxed_vars_in_expr(u, out);
            }
            collect_nested_closure_boxed_vars_in_stmts(body, out);
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_nested_closure_boxed_vars_in_expr(condition, out);
            collect_nested_closure_boxed_vars_in_stmts(body, out);
        }
        Stmt::Try { body, catch, finally } => {
            collect_nested_closure_boxed_vars_in_stmts(body, out);
            if let Some(c) = catch {
                collect_nested_closure_boxed_vars_in_stmts(&c.body, out);
            }
            if let Some(f) = finally {
                collect_nested_closure_boxed_vars_in_stmts(f, out);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_nested_closure_boxed_vars_in_expr(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_nested_closure_boxed_vars_in_expr(t, out);
                }
                collect_nested_closure_boxed_vars_in_stmts(&case.body, out);
            }
        }
        _ => {}
    }
}

fn collect_nested_closure_boxed_vars_in_expr(
    expr: &perry_hir::Expr,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Expr;
    match expr {
        Expr::Closure { body, .. } => {
            // Each closure is its own lexical scope — run the scope
            // analysis on the body, then recurse into any closures
            // that appear inside it.
            let inner = collect_boxed_vars_scope(body);
            out.extend(inner);
            collect_nested_closure_boxed_vars_in_stmts(body, out);
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            collect_nested_closure_boxed_vars_in_expr(left, out);
            collect_nested_closure_boxed_vars_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => {
            collect_nested_closure_boxed_vars_in_expr(operand, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_nested_closure_boxed_vars_in_expr(callee, out);
            for a in args {
                collect_nested_closure_boxed_vars_in_expr(a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_nested_closure_boxed_vars_in_expr(a, out);
            }
        }
        Expr::Array(items) => {
            for i in items {
                collect_nested_closure_boxed_vars_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_nested_closure_boxed_vars_in_expr(v, out);
            }
        }
        Expr::LocalSet(_, v) => {
            collect_nested_closure_boxed_vars_in_expr(v, out);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_nested_closure_boxed_vars_in_expr(condition, out);
            collect_nested_closure_boxed_vars_in_expr(then_expr, out);
            collect_nested_closure_boxed_vars_in_expr(else_expr, out);
        }
        Expr::IndexGet { object, index } => {
            collect_nested_closure_boxed_vars_in_expr(object, out);
            collect_nested_closure_boxed_vars_in_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_nested_closure_boxed_vars_in_expr(object, out);
            collect_nested_closure_boxed_vars_in_expr(index, out);
            collect_nested_closure_boxed_vars_in_expr(value, out);
        }
        Expr::PropertyGet { object, .. } => {
            collect_nested_closure_boxed_vars_in_expr(object, out);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_nested_closure_boxed_vars_in_expr(object, out);
            collect_nested_closure_boxed_vars_in_expr(value, out);
        }
        Expr::Await(inner) => {
            collect_nested_closure_boxed_vars_in_expr(inner, out);
        }
        Expr::ArrayPush { value, .. } => {
            collect_nested_closure_boxed_vars_in_expr(value, out);
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_nested_closure_boxed_vars_in_expr(array, out);
            collect_nested_closure_boxed_vars_in_expr(callback, out);
        }
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            collect_nested_closure_boxed_vars_in_expr(array, out);
            collect_nested_closure_boxed_vars_in_expr(callback, out);
            if let Some(init) = initial {
                collect_nested_closure_boxed_vars_in_expr(init, out);
            }
        }
        _ => {}
    }
}

/// Collect LocalIds declared inside the init slot of any `for` loop
/// anywhere in the given statements. Used by `collect_boxed_vars` to
/// exclude loop counters from the boxing set (they follow fresh-
/// binding-per-iteration semantics under let scoping).
/// Detect the self-recursive closure pattern: `let fib = (n) => fib(n-1)`.
/// When a Stmt::Let's Closure init captures the Let's own id, that id must
/// be boxed so the closure body can read the live value instead of the
/// stale 0.0 that was in the slot at capture time.
fn collect_self_recursive_closure_ids(
    stmts: &[perry_hir::Stmt],
    closure_refs: &HashSet<u32>,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        if let Stmt::Let { id, init: Some(init_expr), .. } = s {
            // Check if the init is a Closure whose body references
            // this same id. We don't need to walk the full body —
            // just check if the id is in the already-computed
            // closure_refs set (which includes all ids referenced
            // from any closure body in these stmts).
            if matches!(init_expr, perry_hir::Expr::Closure { .. }) {
                if closure_refs.contains(id) {
                    out.insert(*id);
                }
            }
        }
        // Recurse into nested blocks.
        match s {
            Stmt::If { then_branch, else_branch, .. } => {
                collect_self_recursive_closure_ids(then_branch, closure_refs, out);
                if let Some(eb) = else_branch {
                    collect_self_recursive_closure_ids(eb, closure_refs, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_self_recursive_closure_ids(
                        std::slice::from_ref(init_stmt.as_ref()),
                        closure_refs,
                        out,
                    );
                }
                collect_self_recursive_closure_ids(body, closure_refs, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_self_recursive_closure_ids(body, closure_refs, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_self_recursive_closure_ids(body, closure_refs, out);
                if let Some(c) = catch {
                    collect_self_recursive_closure_ids(&c.body, closure_refs, out);
                }
                if let Some(f) = finally {
                    collect_self_recursive_closure_ids(f, closure_refs, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_self_recursive_closure_ids(&case.body, closure_refs, out);
                }
            }
            _ => {}
        }
    }
}

fn collect_for_init_ids(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_let_ids(std::slice::from_ref(init_stmt.as_ref()), out);
                }
                collect_for_init_ids(body, out);
            }
            Stmt::If { then_branch, else_branch, .. } => {
                collect_for_init_ids(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_for_init_ids(eb, out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_for_init_ids(body, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_for_init_ids(body, out);
                if let Some(c) = catch {
                    collect_for_init_ids(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_for_init_ids(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_for_init_ids(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Walk statements and, for every Closure expression encountered,
/// collect the ids that its body reads and writes. Skips nested
/// closures' bodies (those are analyzed independently when they're
/// compiled).
fn collect_closure_refs_and_writes_in_stmts(
    stmts: &[perry_hir::Stmt],
    refs: &mut HashSet<u32>,
    writes: &mut HashSet<u32>,
) {
    for s in stmts {
        collect_closure_refs_and_writes_in_stmt(s, refs, writes);
    }
}

fn collect_closure_refs_and_writes_in_stmt(
    stmt: &perry_hir::Stmt,
    refs: &mut HashSet<u32>,
    writes: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            collect_closure_refs_and_writes_in_expr(e, refs, writes);
        }
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_closure_refs_and_writes_in_expr(e, refs, writes);
            }
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_closure_refs_and_writes_in_expr(e, refs, writes);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            collect_closure_refs_and_writes_in_expr(condition, refs, writes);
            collect_closure_refs_and_writes_in_stmts(then_branch, refs, writes);
            if let Some(eb) = else_branch {
                collect_closure_refs_and_writes_in_stmts(eb, refs, writes);
            }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init {
                collect_closure_refs_and_writes_in_stmt(i, refs, writes);
            }
            if let Some(c) = condition {
                collect_closure_refs_and_writes_in_expr(c, refs, writes);
            }
            if let Some(u) = update {
                collect_closure_refs_and_writes_in_expr(u, refs, writes);
            }
            collect_closure_refs_and_writes_in_stmts(body, refs, writes);
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_refs_and_writes_in_expr(condition, refs, writes);
            collect_closure_refs_and_writes_in_stmts(body, refs, writes);
        }
        Stmt::Try { body, catch, finally } => {
            collect_closure_refs_and_writes_in_stmts(body, refs, writes);
            if let Some(c) = catch {
                collect_closure_refs_and_writes_in_stmts(&c.body, refs, writes);
            }
            if let Some(f) = finally {
                collect_closure_refs_and_writes_in_stmts(f, refs, writes);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_closure_refs_and_writes_in_expr(discriminant, refs, writes);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_closure_refs_and_writes_in_expr(t, refs, writes);
                }
                collect_closure_refs_and_writes_in_stmts(&case.body, refs, writes);
            }
        }
        _ => {}
    }
}

fn collect_closure_refs_and_writes_in_expr(
    expr: &perry_hir::Expr,
    refs: &mut HashSet<u32>,
    writes: &mut HashSet<u32>,
) {
    use perry_hir::Expr;
    // Note: this walker only RECURSES into closures (see the
    // Expr::Closure case below). For top-level (non-closure) stmts,
    // we do not add writes/refs here — the `collect_outer_writes_*`
    // walker handles that path. Keeping those concerns separate
    // prevents the `arr.push(x)` at top level from being treated
    // as a "closure capture write" and triggering false-positive
    // boxing of a non-captured variable.
    match expr {
        Expr::Closure { body, .. } => {
            // Collect every LocalGet/LocalSet/Update ref inside the
            // closure body. Nested closures inside this body will
            // also contribute their refs.
            collect_ref_ids_in_stmts(body, refs);
            collect_write_ids_in_stmts(body, writes);
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            collect_closure_refs_and_writes_in_expr(left, refs, writes);
            collect_closure_refs_and_writes_in_expr(right, refs, writes);
        }
        Expr::Unary { operand, .. } => {
            collect_closure_refs_and_writes_in_expr(operand, refs, writes);
        }
        // Update at this level is outside any closure body — the walker only
        // recurses INTO closures via the Closure arm below, so seeing an
        // Update here means it's a top-level mutation, not a captured one.
        // The previous implementation inserted unconditionally, which made
        // every plain `for (let i = ...; ...; i++)` body's `i` look like a
        // closure-captured-and-mutated var and forced a box allocation. The
        // box turned the loop counter into a `bl js_box_get` / `bl js_box_set`
        // pair per iteration even when no closure existed in the function.
        // Drop the insertion; the captured-inside-closure case is still
        // handled by `collect_write_ids_in_stmts` triggered from the
        // Expr::Closure arm above.
        Expr::Update { .. } => {}
        Expr::Call { callee, args, .. } => {
            collect_closure_refs_and_writes_in_expr(callee, refs, writes);
            for a in args {
                collect_closure_refs_and_writes_in_expr(a, refs, writes);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_closure_refs_and_writes_in_expr(a, refs, writes);
            }
        }
        Expr::Array(items) => {
            for i in items {
                collect_closure_refs_and_writes_in_expr(i, refs, writes);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_closure_refs_and_writes_in_expr(v, refs, writes);
            }
        }
        Expr::LocalSet(_, v) => {
            collect_closure_refs_and_writes_in_expr(v, refs, writes);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_closure_refs_and_writes_in_expr(condition, refs, writes);
            collect_closure_refs_and_writes_in_expr(then_expr, refs, writes);
            collect_closure_refs_and_writes_in_expr(else_expr, refs, writes);
        }
        Expr::IndexGet { object, index } => {
            collect_closure_refs_and_writes_in_expr(object, refs, writes);
            collect_closure_refs_and_writes_in_expr(index, refs, writes);
        }
        Expr::IndexSet { object, index, value } => {
            collect_closure_refs_and_writes_in_expr(object, refs, writes);
            collect_closure_refs_and_writes_in_expr(index, refs, writes);
            collect_closure_refs_and_writes_in_expr(value, refs, writes);
        }
        Expr::PropertyGet { object, .. } => {
            collect_closure_refs_and_writes_in_expr(object, refs, writes);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_closure_refs_and_writes_in_expr(object, refs, writes);
            collect_closure_refs_and_writes_in_expr(value, refs, writes);
        }
        // Expr::ArrayPush carries the value as an Expr; recurse in
        // so a closure literal passed as the push argument (e.g.
        // `fns.push(() => x)`) is visited and its captured ids
        // contribute to refs.
        Expr::ArrayPush { value, .. } => {
            collect_closure_refs_and_writes_in_expr(value, refs, writes);
        }
        // Array HOF expressions that carry a callback — the callback
        // is often a Closure whose body may capture and mutate outer
        // variables. Without walking these, mutable captures inside
        // arr.forEach/map/filter/flatMap callbacks aren't detected
        // and don't get boxed.
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_refs_and_writes_in_expr(array, refs, writes);
            collect_closure_refs_and_writes_in_expr(callback, refs, writes);
        }
        Expr::ArrayReduce { array, callback, initial }
        | Expr::ArrayReduceRight { array, callback, initial } => {
            collect_closure_refs_and_writes_in_expr(array, refs, writes);
            collect_closure_refs_and_writes_in_expr(callback, refs, writes);
            if let Some(init) = initial {
                collect_closure_refs_and_writes_in_expr(init, refs, writes);
            }
        }
        _ => {}
    }
}

/// Collect LocalIds that are written (LocalSet or Update) anywhere in
/// the given statements, OUTSIDE of any closure bodies. Used to
/// determine whether an outer-scope var is being mutated, which
/// together with capture triggers boxing.
fn collect_outer_writes_in_stmts(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
) {
    for s in stmts {
        collect_outer_writes_in_stmt(s, out);
    }
}

fn collect_outer_writes_in_stmt(
    stmt: &perry_hir::Stmt,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => collect_outer_writes_in_expr(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_outer_writes_in_expr(e, out);
            }
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_outer_writes_in_expr(e, out);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            collect_outer_writes_in_expr(condition, out);
            collect_outer_writes_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                collect_outer_writes_in_stmts(eb, out);
            }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init {
                collect_outer_writes_in_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_outer_writes_in_expr(c, out);
            }
            if let Some(u) = update {
                collect_outer_writes_in_expr(u, out);
            }
            collect_outer_writes_in_stmts(body, out);
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_outer_writes_in_expr(condition, out);
            collect_outer_writes_in_stmts(body, out);
        }
        Stmt::Try { body, catch, finally } => {
            collect_outer_writes_in_stmts(body, out);
            if let Some(c) = catch {
                collect_outer_writes_in_stmts(&c.body, out);
            }
            if let Some(f) = finally {
                collect_outer_writes_in_stmts(f, out);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_outer_writes_in_expr(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_outer_writes_in_expr(t, out);
                }
                collect_outer_writes_in_stmts(&case.body, out);
            }
        }
        _ => {}
    }
}

fn collect_outer_writes_in_expr(
    expr: &perry_hir::Expr,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Expr;
    // Same mutating-method detection as the closure walker.
    if let Expr::Call { callee, .. } = expr {
        if let Expr::PropertyGet { object, property } = callee.as_ref() {
            if let Expr::LocalGet(id) = object.as_ref() {
                // Array-specific mutating methods only. "add"/"set"/
                // "delete"/"clear" collide with user-defined custom
                // methods (e.g. `a.add(x)` on a plain object literal
                // isn't necessarily a Set), so we keep them out of
                // this list to avoid false-positive box promotion.
                if matches!(
                    property.as_str(),
                    "push" | "pop" | "shift" | "unshift" | "splice"
                        | "sort" | "reverse" | "fill" | "copyWithin"
                ) {
                    out.insert(*id);
                }
            }
        }
    }
    if let Expr::ArrayPush { array_id, .. } = expr {
        out.insert(*array_id);
    }
    match expr {
        // STOP recursing into closures — those are "inside"; we only
        // collect outer-scope writes here.
        Expr::Closure { .. } => {}
        Expr::LocalSet(id, v) => {
            out.insert(*id);
            collect_outer_writes_in_expr(v, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            collect_outer_writes_in_expr(left, out);
            collect_outer_writes_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_outer_writes_in_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_outer_writes_in_expr(callee, out);
            for a in args {
                collect_outer_writes_in_expr(a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_outer_writes_in_expr(a, out);
            }
        }
        Expr::Array(items) => {
            for i in items {
                collect_outer_writes_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_outer_writes_in_expr(v, out);
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_outer_writes_in_expr(condition, out);
            collect_outer_writes_in_expr(then_expr, out);
            collect_outer_writes_in_expr(else_expr, out);
        }
        Expr::IndexGet { object, index } => {
            collect_outer_writes_in_expr(object, out);
            collect_outer_writes_in_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_outer_writes_in_expr(object, out);
            collect_outer_writes_in_expr(index, out);
            collect_outer_writes_in_expr(value, out);
        }
        Expr::PropertyGet { object, .. } => {
            collect_outer_writes_in_expr(object, out);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_outer_writes_in_expr(object, out);
            collect_outer_writes_in_expr(value, out);
        }
        _ => {}
    }
}

/// Collect LocalIds that are written (LocalSet or Update) anywhere in
/// the given statements, INCLUDING inside nested closures. Used to
/// detect whether a local is ever mutated — the "is this captured +
/// mutated" gate for boxing.
fn collect_write_ids_in_stmts(
    stmts: &[perry_hir::Stmt],
    out: &mut HashSet<u32>,
) {
    for s in stmts {
        collect_write_ids_in_stmt(s, out);
    }
}

fn collect_write_ids_in_stmt(
    stmt: &perry_hir::Stmt,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Expr(e) | Stmt::Throw(e) => collect_write_ids_in_expr(e, out),
        Stmt::Return(opt) => {
            if let Some(e) = opt {
                collect_write_ids_in_expr(e, out);
            }
        }
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_write_ids_in_expr(e, out);
            }
        }
        Stmt::If { condition, then_branch, else_branch } => {
            collect_write_ids_in_expr(condition, out);
            collect_write_ids_in_stmts(then_branch, out);
            if let Some(eb) = else_branch {
                collect_write_ids_in_stmts(eb, out);
            }
        }
        Stmt::For { init, condition, update, body } => {
            if let Some(i) = init {
                collect_write_ids_in_stmt(i, out);
            }
            if let Some(c) = condition {
                collect_write_ids_in_expr(c, out);
            }
            if let Some(u) = update {
                collect_write_ids_in_expr(u, out);
            }
            collect_write_ids_in_stmts(body, out);
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_write_ids_in_expr(condition, out);
            collect_write_ids_in_stmts(body, out);
        }
        Stmt::Try { body, catch, finally } => {
            collect_write_ids_in_stmts(body, out);
            if let Some(c) = catch {
                collect_write_ids_in_stmts(&c.body, out);
            }
            if let Some(f) = finally {
                collect_write_ids_in_stmts(f, out);
            }
        }
        Stmt::Switch { discriminant, cases } => {
            collect_write_ids_in_expr(discriminant, out);
            for case in cases {
                if let Some(t) = &case.test {
                    collect_write_ids_in_expr(t, out);
                }
                collect_write_ids_in_stmts(&case.body, out);
            }
        }
        _ => {}
    }
}

fn collect_write_ids_in_expr(
    expr: &perry_hir::Expr,
    out: &mut HashSet<u32>,
) {
    use perry_hir::Expr;
    // Mutating method calls count as writes on the receiver.
    if let Expr::Call { callee, .. } = expr {
        if let Expr::PropertyGet { object, property } = callee.as_ref() {
            if let Expr::LocalGet(id) = object.as_ref() {
                // Array-specific mutating methods only. "add"/"set"/
                // "delete"/"clear" collide with user-defined custom
                // methods (e.g. `a.add(x)` on a plain object literal
                // isn't necessarily a Set), so we keep them out of
                // this list to avoid false-positive box promotion.
                if matches!(
                    property.as_str(),
                    "push" | "pop" | "shift" | "unshift" | "splice"
                        | "sort" | "reverse" | "fill" | "copyWithin"
                ) {
                    out.insert(*id);
                }
            }
        }
    }
    if let Expr::ArrayPush { array_id, .. } = expr {
        out.insert(*array_id);
    }
    match expr {
        Expr::LocalSet(id, v) => {
            out.insert(*id);
            collect_write_ids_in_expr(v, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Closure { body, .. } => collect_write_ids_in_stmts(body, out),
        Expr::Binary { left, right, .. }
        | Expr::Logical { left, right, .. }
        | Expr::Compare { left, right, .. } => {
            collect_write_ids_in_expr(left, out);
            collect_write_ids_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_write_ids_in_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_write_ids_in_expr(callee, out);
            for a in args {
                collect_write_ids_in_expr(a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_write_ids_in_expr(a, out);
            }
        }
        Expr::Array(items) => {
            for i in items {
                collect_write_ids_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_write_ids_in_expr(v, out);
            }
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_write_ids_in_expr(condition, out);
            collect_write_ids_in_expr(then_expr, out);
            collect_write_ids_in_expr(else_expr, out);
        }
        Expr::IndexGet { object, index } => {
            collect_write_ids_in_expr(object, out);
            collect_write_ids_in_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_write_ids_in_expr(object, out);
            collect_write_ids_in_expr(index, out);
            collect_write_ids_in_expr(value, out);
        }
        Expr::PropertyGet { object, .. } => {
            collect_write_ids_in_expr(object, out);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_write_ids_in_expr(object, out);
            collect_write_ids_in_expr(value, out);
        }
        _ => {}
    }
}

/// Walk statements and collect every `Stmt::Let`'s (id, type) pair
/// into the given map. Used to build a module-wide LocalId → Type
/// map so closure bodies can learn captured-var types without
/// having a handle on the enclosing function's context.
pub(crate) fn collect_let_types_in_stmts(
    stmts: &[perry_hir::Stmt],
    out: &mut HashMap<u32, perry_types::Type>,
) {
    use perry_hir::Stmt;
    for s in stmts {
        match s {
            Stmt::Let { id, ty, init, .. } => {
                // Refine Any-typed lets from the init if possible,
                // so closures inherit the right type.
                let refined_ty = if matches!(ty, perry_types::Type::Any) {
                    init.as_ref()
                        .and_then(refine_type_from_init_simple)
                        .unwrap_or_else(|| ty.clone())
                } else {
                    ty.clone()
                };
                out.insert(*id, refined_ty);
            }
            Stmt::If { then_branch, else_branch, .. } => {
                collect_let_types_in_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_let_types_in_stmts(eb, out);
                }
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_let_types_in_stmts(std::slice::from_ref(init_stmt.as_ref()), out);
                }
                collect_let_types_in_stmts(body, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_types_in_stmts(body, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_let_types_in_stmts(body, out);
                if let Some(c) = catch {
                    collect_let_types_in_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_let_types_in_stmts(f, out);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_let_types_in_stmts(&case.body, out);
                }
            }
            _ => {}
        }
        // Walk closure bodies nested in the statements so their
        // inner lets are also registered.
        if let Stmt::Expr(e)
        | Stmt::Return(Some(e))
        | Stmt::Let { init: Some(e), .. } = s
        {
            collect_closure_let_types_in_expr(e, out);
        }
    }
}

fn collect_closure_let_types_in_expr(
    expr: &perry_hir::Expr,
    out: &mut HashMap<u32, perry_types::Type>,
) {
    use perry_hir::Expr;
    match expr {
        Expr::Closure { params, body, .. } => {
            for p in params {
                out.insert(p.id, p.ty.clone());
            }
            collect_let_types_in_stmts(body, out);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_let_types_in_expr(left, out);
            collect_closure_let_types_in_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_let_types_in_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_let_types_in_expr(callee, out);
            for a in args {
                collect_closure_let_types_in_expr(a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_closure_let_types_in_expr(a, out);
            }
        }
        Expr::Array(items) => {
            for i in items {
                collect_closure_let_types_in_expr(i, out);
            }
        }
        Expr::Object(props) => {
            for (_, v) in props {
                collect_closure_let_types_in_expr(v, out);
            }
        }
        Expr::LocalSet(_, v) => collect_closure_let_types_in_expr(v, out),
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_closure_let_types_in_expr(condition, out);
            collect_closure_let_types_in_expr(then_expr, out);
            collect_closure_let_types_in_expr(else_expr, out);
        }
        Expr::IndexGet { object, index } => {
            collect_closure_let_types_in_expr(object, out);
            collect_closure_let_types_in_expr(index, out);
        }
        Expr::PropertyGet { object, .. } => {
            collect_closure_let_types_in_expr(object, out);
        }
        _ => {}
    }
}

/// Mirror of `expr::refine_type_from_init` but without a FnCtx —
/// used at module-level type collection time before any FnCtx
/// exists. Conservative: only refines a small set of expression
/// shapes where the type is obvious from the AST alone.
fn refine_type_from_init_simple(init: &perry_hir::Expr) -> Option<perry_types::Type> {
    use perry_hir::Expr;
    use perry_types::Type;
    match init {
        Expr::Array(_) | Expr::ArraySpread(_) => Some(Type::Array(Box::new(Type::Any))),
        Expr::ArraySlice { .. }
        | Expr::ArrayMap { .. }
        | Expr::ArrayFilter { .. }
        | Expr::ArrayFlat { .. }
        | Expr::ArrayFlatMap { .. }
        | Expr::ObjectKeys(_)
        | Expr::ObjectValues(_)
        | Expr::ObjectEntries(_)
        | Expr::ArrayEntries { .. }
        | Expr::ArrayKeys { .. }
        | Expr::ArrayValues { .. }
        | Expr::StringMatch { .. }
        | Expr::StringMatchAll { .. } => Some(Type::Array(Box::new(Type::Any))),
        Expr::String(_) | Expr::ArrayJoin { .. } | Expr::StringCoerce(_) => Some(Type::String),
        Expr::Bool(_) => Some(Type::Boolean),
        Expr::BigInt(_) | Expr::BigIntCoerce(_) => Some(Type::BigInt),
        Expr::New { class_name, .. } => Some(Type::Named(class_name.clone())),
        // `const ta = new Int32Array(n)` — refine to Named("Int32Array") so
        // that `.length` and method dispatch use the typed-array fast paths.
        Expr::TypedArrayNew { kind, .. } => {
            let name = match *kind {
                0 => "Int8Array",
                1 => "Uint8Array",
                2 => "Int16Array",
                3 => "Uint16Array",
                4 => "Int32Array",
                5 => "Uint32Array",
                6 => "Float32Array",
                7 => "Float64Array",
                8 => "Uint8ClampedArray",
                _ => return None,
            };
            Some(Type::Named(name.to_string()))
        }
        _ => None,
    }
}
