//! Beta-reduce closure-literal locals that are only ever called.
//!
//! Inlining a helper that takes a callback leaves this shape behind:
//!
//! ```text
//! let exists = (id) => this.exists(id);          // the caller's argument
//! if (exists === undefined) exists = () => true; // the callee's default
//! …
//! if (!exists(entityId)) throw …                 // the callee's only use
//! ```
//!
//! Every call of `exists` still allocates the closure, dispatches through
//! `js_closure_call1`, and saves/restores the implicit `this` around it — on
//! the ECS `world.set` path that was three runtime calls per invocation for a
//! callback whose body is one expression. Nothing about the closure is
//! observable: it never escapes, is never compared, and is never reassigned
//! (the default guard cannot fire, because a closure literal is not
//! `undefined`).
//!
//! This pass runs after the inliner. For each statement list it:
//!
//! 1. removes `if (x === undefined) x = …` guards whose binding was
//!    initialized with a literal that is never `undefined` and is written
//!    nowhere else — the inliner's default-parameter expansion after a
//!    supplied argument;
//! 2. finds `let f = <arrow literal>` where the arrow is synchronous, has
//!    only plain parameters, none of which the body writes, no mutable
//!    captures, no `new.target` capture, and a single `return <expr>` body,
//!    and where every use of `f` in the rest of the list is a direct call
//!    with exactly the declared number of stable arguments;
//! 3. replaces each such call with the arrow's return expression, parameters
//!    substituted by the arguments, and deletes the `let`.
//!
//! Substitution puts the argument expression where the body reads the
//! parameter, so it is only the call's meaning when the argument has one
//! value from the call to the end of the body and no effect of its own: a
//! literal, or a read of a local the module census (`local_occurrences`)
//! shows is never written. A read of a local that anything writes could see
//! a write the body makes before the read (`f = (a) => bump() + a; f(m)`,
//! `bump` writing `m`), and a global read is not known to be unwritten. A
//! parameter the body writes is rejected too: substitution renames the
//! write onto the argument's local, which would change the caller's binding
//! (`f = (a) => (a = a + 10); f(x)` must leave `x` alone).
//!
//! Because the arrow is lexically scoped, `this`, captured locals and
//! `arguments` inside its body already denote the enclosing function's
//! bindings, so the substituted expression is valid exactly where the call
//! was. Any other use — a read that is not a callee, a write, a capture by a
//! nested closure, a self-reference — leaves the binding untouched.

use std::collections::HashMap;

use perry_hir::types::LocalId;
use perry_hir::walker::{walk_expr_children, walk_expr_children_mut};
use perry_hir::{CompareOp, Expr, Function, Module, Stmt};

use crate::inline::substitute_locals;
use crate::local_occurrences::count_module;

/// Module-wide write counts (`LocalSet` / `Update` targets) from the shared
/// census. A local absent from it is never written. The rewrite keeps that
/// true: it copies bodies that write only locals already counted, and it
/// never reduces an arrow whose parameter is written.
type Writes = HashMap<LocalId, usize>;

pub fn run(module: &mut Module) {
    let mut next_local_id = crate::generator::compute_max_local_id(module).saturating_add(1);
    let writes = count_module(module, true);
    for f in &mut module.functions {
        run_function(f, &writes, &mut next_local_id);
    }
    for c in &mut module.classes {
        if let Some(ctor) = &mut c.constructor {
            run_function(ctor, &writes, &mut next_local_id);
        }
        for m in &mut c.methods {
            run_function(m, &writes, &mut next_local_id);
        }
        for m in &mut c.static_methods {
            run_function(m, &writes, &mut next_local_id);
        }
        for (_, g) in &mut c.getters {
            run_function(g, &writes, &mut next_local_id);
        }
        for (_, s) in &mut c.setters {
            run_function(s, &writes, &mut next_local_id);
        }
    }
}

fn run_function(f: &mut Function, writes: &Writes, next_local_id: &mut LocalId) {
    // The async/generator transforms rewrite these bodies into state machines
    // whose locals are boxed cells; keep the shapes they expect.
    if f.is_async || f.is_generator {
        return;
    }
    process_stmts(&mut f.body, writes, next_local_id);
}

fn process_stmts(stmts: &mut Vec<Stmt>, writes: &Writes, next_local_id: &mut LocalId) {
    // Inner lists and closure bodies first, so a nested helper is reduced in
    // its own scope before the enclosing list is examined.
    for s in stmts.iter_mut() {
        for inner in nested_stmt_lists(s) {
            process_stmts(inner, writes, next_local_id);
        }
        for_each_expr_in_stmt_mut(s, &mut |e| process_closure_bodies(e, writes, next_local_id));
    }

    remove_dead_default_guards(stmts);

    let mut i = 0;
    while i < stmts.len() {
        let candidate = match &stmts[i] {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => arrow_candidate(*id, init, writes),
            _ => None,
        };
        let Some((id, params, body_expr)) = candidate else {
            i += 1;
            continue;
        };
        // An inlined callee binds its callback parameter as a copy local
        // (`let exists' = exists`); follow such copies so the calls through
        // them count as calls of the closure.
        let set = collect_aliases(&stmts[i + 1..], id);
        // Forward captures and reads can precede the declaration. Removing
        // its initializer leaves those live boxes uninitialized even though
        // every later use is an inlineable call (#9721). Earlier calls must
        // also retain their original TDZ behavior.
        let mut earlier_uses = Uses::new(params.len(), writes);
        for s in &stmts[..i] {
            collect_uses_in_stmt(s, &set, &mut earlier_uses);
        }
        if earlier_uses.other || earlier_uses.calls != 0 {
            i += 1;
            continue;
        }
        let mut uses = Uses::new(params.len(), writes);
        for s in &stmts[i + 1..] {
            collect_uses_in_stmt(s, &set, &mut uses);
        }
        if uses.other || uses.calls == 0 || uses.bad_arity {
            i += 1;
            continue;
        }
        let mut tail: Vec<Stmt> = stmts.split_off(i + 1);
        for s in tail.iter_mut() {
            rewrite_calls_in_stmt(s, &set, &params, &body_expr, next_local_id);
        }
        remove_alias_lets(&mut tail, &set);
        // Drop the closure's own `let` and splice the rewritten tail back.
        stmts.truncate(i);
        stmts.extend(tail);
        // Do not advance: the list shifted, and the statement now at `i` may
        // itself be a candidate.
    }
}

fn process_closure_bodies(expr: &mut Expr, writes: &Writes, next_local_id: &mut LocalId) {
    if let Expr::Closure { body, .. } = expr {
        process_stmts(body, writes, next_local_id);
    }
    walk_expr_children_mut(expr, &mut |child| {
        process_closure_bodies(child, writes, next_local_id)
    });
}

/// `let f = (a, b) => <expr>` with the admission rules from the module doc.
/// Returns the binding, its parameter ids, and the return expression.
fn arrow_candidate(
    id: LocalId,
    init: &Expr,
    writes: &Writes,
) -> Option<(LocalId, Vec<LocalId>, Expr)> {
    let Expr::Closure {
        params,
        body,
        captures,
        mutable_captures,
        captures_new_target,
        is_arrow,
        is_async,
        is_generator,
        ..
    } = init
    else {
        return None;
    };
    if !*is_arrow
        || *is_async
        || *is_generator
        || *captures_new_target
        || !mutable_captures.is_empty()
        || captures.contains(&id)
        || params
            .iter()
            .any(|p| p.default.is_some() || p.is_rest || p.arguments_object.is_some())
        // Parameter ids are the arrow's own, so a census write is a write
        // in this body.
        || params.iter().any(|p| writes.contains_key(&p.id))
    {
        return None;
    }
    let [Stmt::Return(Some(expr))] = body.as_slice() else {
        return None;
    };
    // #10567: a param captured by a closure NESTED inside `expr` (e.g. `(f,
    // isOpt) => arr.forEach(([k, v]) => check(k, v, isOpt))`) cannot be
    // beta-reduced the way a plain read can. `rewrite_calls` clones
    // `body_expr` fresh per call site and hands the clone to
    // `substitute_locals`, which — for a nested `Expr::Closure` — bakes a
    // non-`LocalGet` argument straight into that closure's body and drops
    // it from its `captures` list (see `inline/substitute.rs`'s
    // `Expr::Closure` arm), but never mints a fresh `func_id` for the
    // rewritten closure literal. Codegen compiles exactly one body per
    // `func_id` (whichever occurrence its module-wide closure scan sees
    // first), so every call site's clone of the nested closure keeps
    // sharing the SAME `func_id` — once there is more than one call site,
    // only the first-seen clone's baked-in argument is ever compiled, and
    // every other call silently runs it too. Bail out when any param is
    // captured by a nested closure so such an arrow is left as a real,
    // per-call closure — each invocation then creates its own closure
    // instance whose nested callback correctly captures that call's
    // argument by reference (the existing, non-beta-reduced path already
    // gets this right).
    let mut closure_captured_params = std::collections::HashSet::new();
    crate::inline::collect_closure_captured_local_ids(body, &mut closure_captured_params);
    if params
        .iter()
        .any(|p| closure_captured_params.contains(&p.id))
    {
        return None;
    }
    Some((id, params.iter().map(|p| p.id).collect(), expr.clone()))
}

/// An argument whose value is the same wherever the substituted body reads
/// it and whose evaluation has no effect, so substitution may move, copy or
/// drop it: a literal, or a read of a local nothing in the module writes.
fn is_stable_arg(expr: &Expr, writes: &Writes) -> bool {
    match expr {
        Expr::Integer(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Null
        | Expr::Undefined => true,
        Expr::LocalGet(x) => !writes.contains_key(x),
        _ => false,
    }
}

/// `id` plus every local that is a plain copy of it (`let y = id;`,
/// transitively), searched through nested statement lists.
fn collect_aliases(stmts: &[Stmt], id: LocalId) -> Vec<LocalId> {
    let mut set = vec![id];
    loop {
        let before = set.len();
        for s in stmts {
            collect_alias_lets_in_stmt(s, &mut set);
        }
        if set.len() == before {
            return set;
        }
    }
}

fn collect_alias_lets_in_stmt(stmt: &Stmt, set: &mut Vec<LocalId>) {
    if let Stmt::Let {
        id,
        init: Some(Expr::LocalGet(src)),
        ..
    } = stmt
    {
        if set.contains(src) && !set.contains(id) {
            set.push(*id);
        }
    }
    for inner in nested_stmt_lists_ref(stmt) {
        for s in inner {
            collect_alias_lets_in_stmt(s, set);
        }
    }
}

fn remove_alias_lets(stmts: &mut Vec<Stmt>, set: &[LocalId]) {
    stmts.retain(|s| {
        !matches!(
            s,
            Stmt::Let { id, init: Some(Expr::LocalGet(src)), .. }
                if set.contains(id) && set.contains(src)
        )
    });
    for s in stmts.iter_mut() {
        remove_alias_lets_in_stmt(s, set);
    }
}

fn remove_alias_lets_in_stmt(stmt: &mut Stmt, set: &[LocalId]) {
    for inner in nested_stmt_lists(stmt) {
        remove_alias_lets(inner, set);
    }
}

struct Uses<'a> {
    /// The arrow's declared parameter count. A call with a different
    /// argument count is not rewritten by `rewrite_calls`, so it must veto
    /// the reduction here; otherwise the `let` is deleted while that call
    /// still reads it (a helper called as `c(() => Symbol(..))` is inlined with its
    /// missing second argument as `undefined`, leaving a call of a deleted
    /// local that lowered to calling the constant `undefined`).
    arity: usize,
    calls: usize,
    bad_arity: bool,
    other: bool,
    writes: &'a Writes,
}

impl<'a> Uses<'a> {
    fn new(arity: usize, writes: &'a Writes) -> Self {
        Uses {
            arity,
            calls: 0,
            bad_arity: false,
            other: false,
            writes,
        }
    }
}

fn collect_uses_in_stmt(stmt: &Stmt, set: &[LocalId], uses: &mut Uses) {
    // The copy that defines an alias is not a use of the closure.
    if let Stmt::Let {
        id,
        init: Some(Expr::LocalGet(src)),
        ..
    } = stmt
    {
        if set.contains(id) && set.contains(src) {
            return;
        }
    }
    for_each_expr_in_stmt(stmt, &mut |e| collect_uses(e, set, uses));
    for inner in nested_stmt_lists_ref(stmt) {
        for s in inner {
            collect_uses_in_stmt(s, set, uses);
        }
    }
}

fn collect_uses(expr: &Expr, set: &[LocalId], uses: &mut Uses) {
    match expr {
        Expr::Call { callee, args, .. } if matches!(callee.as_ref(), Expr::LocalGet(x) if set.contains(x)) =>
        {
            uses.calls += 1;
            if args.len() != uses.arity || !args.iter().all(|a| is_stable_arg(a, uses.writes)) {
                uses.bad_arity = true;
            }
            for a in args {
                collect_uses(a, set, uses);
            }
        }
        Expr::LocalGet(x) if set.contains(x) => uses.other = true,
        Expr::LocalSet(x, value) => {
            if set.contains(x) {
                uses.other = true;
            }
            collect_uses(value, set, uses);
        }
        Expr::Closure { captures, body, .. } => {
            if captures.iter().any(|c| set.contains(c)) {
                uses.other = true;
            }
            for s in body {
                collect_uses_in_stmt(s, set, uses);
            }
            walk_expr_children(expr, &mut |child| collect_uses(child, set, uses));
        }
        _ => walk_expr_children(expr, &mut |child| collect_uses(child, set, uses)),
    }
}

/// Rewrite every call of `id` in `stmt`, including its nested statement lists.
fn rewrite_calls_in_stmt(
    stmt: &mut Stmt,
    set: &[LocalId],
    params: &[LocalId],
    body_expr: &Expr,
    next_local_id: &mut LocalId,
) {
    for_each_expr_in_stmt_mut(stmt, &mut |e| {
        rewrite_calls(e, set, params, body_expr, next_local_id)
    });
    for inner in nested_stmt_lists(stmt) {
        for s in inner.iter_mut() {
            rewrite_calls_in_stmt(s, set, params, body_expr, next_local_id);
        }
    }
}

fn rewrite_calls(
    expr: &mut Expr,
    set: &[LocalId],
    params: &[LocalId],
    body_expr: &Expr,
    next_local_id: &mut LocalId,
) {
    let is_target = matches!(expr, Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::LocalGet(x) if set.contains(x)));
    if is_target {
        let Expr::Call { args, .. } = expr else {
            unreachable!()
        };
        // Arity was checked during collection; a mismatch here means the
        // count changed under us, which cannot happen, but stay conservative.
        if args.len() != params.len() {
            return;
        }
        let mut map: HashMap<LocalId, Expr> = HashMap::with_capacity(params.len());
        for (p, a) in params.iter().zip(args.iter()) {
            map.insert(*p, a.clone());
        }
        let mut replacement = body_expr.clone();
        substitute_locals(&mut replacement, &map, next_local_id);
        *expr = replacement;
        return;
    }
    if let Expr::Closure { body, .. } = expr {
        for s in body.iter_mut() {
            rewrite_calls_in_stmt(s, set, params, body_expr, next_local_id);
        }
    }
    walk_expr_children_mut(expr, &mut |child| {
        rewrite_calls(child, set, params, body_expr, next_local_id)
    });
}

/// Drop `if (x === undefined) x = …;` when `x` was declared in this list with
/// a literal initializer that is never `undefined` and is not written anywhere
/// else in the list (including the guard's own nested lists).
fn remove_dead_default_guards(stmts: &mut Vec<Stmt>) {
    let mut literal_inits: Vec<LocalId> = Vec::new();
    for s in stmts.iter() {
        if let Stmt::Let {
            id,
            init: Some(init),
            ..
        } = s
        {
            if init_is_never_undefined(init) {
                literal_inits.push(*id);
            }
        }
    }
    if literal_inits.is_empty() {
        return;
    }
    // Count writes per binding across the whole list.
    let mut writes: HashMap<LocalId, usize> = HashMap::new();
    for s in stmts.iter() {
        count_writes_in_stmt(s, &mut writes);
    }
    let mut i = 0;
    while i < stmts.len() {
        let guarded = guarded_default_binding(&stmts[i]);
        match guarded {
            Some(id)
                if literal_inits.contains(&id) && writes.get(&id).copied().unwrap_or(0) == 1 =>
            {
                stmts.remove(i);
            }
            _ => i += 1,
        }
    }
}

fn init_is_never_undefined(init: &Expr) -> bool {
    matches!(
        init,
        Expr::Closure { .. }
            | Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Null
    )
}

fn guarded_default_binding(stmt: &Stmt) -> Option<LocalId> {
    let Stmt::If {
        condition,
        then_branch,
        else_branch: None,
    } = stmt
    else {
        return None;
    };
    let Expr::Compare {
        op: CompareOp::Eq,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let Expr::LocalGet(id) = left.as_ref() else {
        return None;
    };
    if !matches!(right.as_ref(), Expr::Undefined) {
        return None;
    }
    match then_branch.as_slice() {
        [Stmt::Expr(Expr::LocalSet(x, _))] if x == id => Some(*id),
        _ => None,
    }
}

fn count_writes_in_stmt(stmt: &Stmt, writes: &mut HashMap<LocalId, usize>) {
    for_each_expr_in_stmt(stmt, &mut |e| count_writes(e, writes));
    for inner in nested_stmt_lists_ref(stmt) {
        for s in inner {
            count_writes_in_stmt(s, writes);
        }
    }
}

fn count_writes(expr: &Expr, writes: &mut HashMap<LocalId, usize>) {
    match expr {
        Expr::LocalSet(x, value) => {
            *writes.entry(*x).or_default() += 1;
            count_writes(value, writes);
        }
        Expr::Update { id, .. } => {
            *writes.entry(*id).or_default() += 1;
        }
        Expr::Closure {
            body,
            mutable_captures,
            ..
        } => {
            // A write inside a nested closure counts against the binding too.
            for id in mutable_captures {
                *writes.entry(*id).or_default() += 1;
            }
            for s in body {
                count_writes_in_stmt(s, writes);
            }
            walk_expr_children(expr, &mut |child| count_writes(child, writes));
        }
        _ => walk_expr_children(expr, &mut |child| count_writes(child, writes)),
    }
}

// ---------------------------------------------------------------------------
// Statement plumbing
// ---------------------------------------------------------------------------

fn for_each_expr_in_stmt(stmt: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(e) => {
            if let Some(e) = e {
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
            if let Some(init) = init {
                for_each_expr_in_stmt(init, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Labeled { body, .. } => for_each_expr_in_stmt(body, f),
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
        Stmt::Try { .. }
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

pub(crate) fn for_each_expr_in_stmt_mut(stmt: &mut Stmt, f: &mut dyn FnMut(&mut Expr)) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                f(e);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => f(e),
        Stmt::Return(e) => {
            if let Some(e) = e {
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
            if let Some(init) = init {
                for_each_expr_in_stmt_mut(init, f);
            }
            if let Some(c) = condition {
                f(c);
            }
            if let Some(u) = update {
                f(u);
            }
        }
        Stmt::Labeled { body, .. } => for_each_expr_in_stmt_mut(body, f),
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
        Stmt::Try { .. }
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}

pub(crate) fn nested_stmt_lists(s: &mut Stmt) -> Vec<&mut Vec<Stmt>> {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => match else_branch {
            Some(e) => vec![then_branch, e],
            None => vec![then_branch],
        },
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => vec![body],
        Stmt::For { body, .. } => vec![body],
        Stmt::Labeled { body, .. } => nested_stmt_lists(body),
        Stmt::Switch { cases, .. } => cases.iter_mut().map(|c| &mut c.body).collect(),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            let mut v = vec![body];
            if let Some(c) = catch {
                v.push(&mut c.body);
            }
            if let Some(f) = finally {
                v.push(f);
            }
            v
        }
        _ => Vec::new(),
    }
}

fn nested_stmt_lists_ref(s: &Stmt) -> Vec<&Vec<Stmt>> {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => match else_branch {
            Some(e) => vec![then_branch, e],
            None => vec![then_branch],
        },
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => vec![body],
        Stmt::For { body, .. } => vec![body],
        Stmt::Labeled { body, .. } => nested_stmt_lists_ref(body),
        Stmt::Switch { cases, .. } => cases.iter().map(|c| &c.body).collect(),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            let mut v = vec![body];
            if let Some(c) = catch {
                v.push(&c.body);
            }
            if let Some(f) = finally {
                v.push(f);
            }
            v
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::types::Type;
    use perry_hir::Param;

    /// Run the list rewrite with the write census of a module whose only
    /// code is `stmts`, as `run` computes it for the whole module.
    fn reduce(stmts: &mut Vec<Stmt>, next: &mut LocalId) {
        let mut module = Module::new("test");
        module.init = stmts.clone();
        let writes = count_module(&module, true);
        process_stmts(stmts, &writes, next);
    }

    fn param(id: LocalId, name: &str) -> Param {
        Param {
            id,
            name: name.to_string(),
            ty: Type::Any,
            default: None,
            decorators: Vec::new(),
            is_rest: false,
            arguments_object: None,
        }
    }

    fn arrow(func_id: u32, params: Vec<Param>, ret: Expr, captures_this: bool) -> Expr {
        Expr::Closure {
            func_id,
            params,
            return_type: Type::Any,
            body: vec![Stmt::Return(Some(ret))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this,
            captures_new_target: false,
            enclosing_class: Some("World".to_string()),
            is_arrow: true,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    fn call_local(id: LocalId, args: Vec<Expr>) -> Expr {
        Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::LocalGet(id)),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    fn this_exists(arg: Expr) -> Expr {
        Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::PropertyGet {
                byte_end: 0,
                object: Box::new(Expr::This),
                property: "exists".to_string(),
                byte_offset: 0,
            }),
            args: vec![arg],
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    fn guard(id: LocalId, default: Expr) -> Stmt {
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(id)),
                right: Box::new(Expr::Undefined),
            },
            then_branch: vec![Stmt::Expr(Expr::LocalSet(id, Box::new(default)))],
            else_branch: None,
        }
    }

    const F: LocalId = 10;
    const P: LocalId = 11;
    const ENTITY: LocalId = 1;

    /// The `world.set` shape after inlining: callback local, dead default
    /// guard, one direct call.
    #[test]
    fn inlines_a_called_arrow_local_and_drops_its_dead_default_guard() {
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "exists".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(arrow(
                    25,
                    vec![param(P, "id")],
                    this_exists(Expr::LocalGet(P)),
                    true,
                )),
            },
            guard(F, arrow(69, vec![], Expr::Bool(true), false)),
            Stmt::If {
                condition: Expr::Unary {
                    op: perry_hir::UnaryOp::Not,
                    operand: Box::new(call_local(F, vec![Expr::LocalGet(ENTITY)])),
                },
                then_branch: vec![Stmt::Throw(Expr::String("missing".into()))],
                else_branch: None,
            },
        ];
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(stmts.len(), 1, "let and guard removed: {stmts:?}");
        let Stmt::If { condition, .. } = &stmts[0] else {
            panic!("expected the if");
        };
        let Expr::Unary { operand, .. } = condition else {
            panic!("expected the negation");
        };
        assert_eq!(
            format!("{operand:?}"),
            format!("{:?}", this_exists(Expr::LocalGet(ENTITY)))
        );
    }

    /// The inliner wraps a callee body in `do { … } while (false)`, so the
    /// only call usually sits in a nested statement list.
    #[test]
    fn rewrites_calls_inside_nested_statement_lists() {
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "exists".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(arrow(
                    25,
                    vec![param(P, "id")],
                    this_exists(Expr::LocalGet(P)),
                    true,
                )),
            },
            Stmt::DoWhile {
                body: vec![Stmt::If {
                    condition: Expr::Unary {
                        op: perry_hir::UnaryOp::Not,
                        operand: Box::new(call_local(F, vec![Expr::LocalGet(ENTITY)])),
                    },
                    then_branch: vec![Stmt::Break],
                    else_branch: None,
                }],
                condition: Expr::Bool(false),
            },
        ];
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(stmts.len(), 1, "{stmts:?}");
        let rendered = format!("{stmts:?}");
        assert!(
            !rendered.contains("LocalGet(10)"),
            "call not rewritten: {rendered}"
        );
        assert!(
            rendered.contains("\"exists\""),
            "body not substituted: {rendered}"
        );
    }

    /// The inliner binds the callee's callback parameter as a copy local
    /// (`let exists' = exists`) inside its `do { … } while (false)` wrapper.
    #[test]
    fn follows_copy_aliases_of_the_callback_and_removes_them() {
        const ALIAS: LocalId = 20;
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "exists".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(arrow(
                    25,
                    vec![param(P, "id")],
                    this_exists(Expr::LocalGet(P)),
                    true,
                )),
            },
            guard(F, arrow(69, vec![], Expr::Bool(true), false)),
            Stmt::DoWhile {
                body: vec![
                    Stmt::Let {
                        id: ALIAS,
                        name: "exists".into(),
                        ty: Type::Any,
                        mutable: false,
                        init: Some(Expr::LocalGet(F)),
                    },
                    Stmt::If {
                        condition: Expr::Unary {
                            op: perry_hir::UnaryOp::Not,
                            operand: Box::new(call_local(ALIAS, vec![Expr::LocalGet(ENTITY)])),
                        },
                        then_branch: vec![Stmt::Break],
                        else_branch: None,
                    },
                ],
                condition: Expr::Bool(false),
            },
        ];
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        let rendered = format!("{stmts:?}");
        assert_eq!(stmts.len(), 1, "{rendered}");
        assert!(!rendered.contains("LocalGet(10)"), "{rendered}");
        assert!(
            !rendered.contains("LocalGet(20)"),
            "alias let/read survived: {rendered}"
        );
        assert!(
            !rendered.contains("Closure"),
            "closure survived: {rendered}"
        );
        assert!(rendered.contains("\"exists\""), "{rendered}");
    }

    #[test]
    fn a_non_call_use_keeps_the_closure() {
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "f".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(1, vec![param(P, "id")], Expr::LocalGet(P), false)),
            },
            // `f` escapes as an argument.
            Stmt::Expr(Expr::Call {
                byte_end: 0,
                callee: Box::new(Expr::GlobalGet(0)),
                args: vec![Expr::LocalGet(F)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Expr(call_local(F, vec![Expr::Integer(1)])),
        ];
        let before = stmts.clone();
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }

    #[test]
    fn a_forward_capture_keeps_the_later_initializer() {
        let mut earlier = arrow(2, Vec::new(), call_local(F, vec![Expr::Integer(1)]), false);
        if let Expr::Closure {
            captures,
            mutable_captures,
            ..
        } = &mut earlier
        {
            captures.push(F);
            mutable_captures.push(F);
        }
        let mut stmts = vec![
            Stmt::PreallocateTdzBoxes(vec![F]),
            Stmt::Expr(earlier),
            Stmt::Let {
                id: F,
                name: "later".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(1, vec![param(P, "value")], Expr::LocalGet(P), false)),
            },
            Stmt::Expr(call_local(F, vec![Expr::Integer(2)])),
        ];
        let before = format!("{stmts:?}");
        reduce(&mut stmts, &mut 100);
        assert_eq!(format!("{stmts:?}"), before);
    }

    #[test]
    fn a_call_before_initialization_keeps_its_tdz_and_later_initializer() {
        let mut stmts = vec![
            Stmt::PreallocateTdzBoxes(vec![F]),
            Stmt::Expr(call_local(F, vec![Expr::Integer(1)])),
            Stmt::Let {
                id: F,
                name: "later".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(1, vec![param(P, "value")], Expr::LocalGet(P), false)),
            },
            Stmt::Expr(call_local(F, vec![Expr::Integer(2)])),
        ];
        let before = format!("{stmts:?}");
        reduce(&mut stmts, &mut 100);
        assert_eq!(format!("{stmts:?}"), before);
    }

    #[test]
    fn a_non_trivial_argument_or_a_capture_by_a_nested_closure_is_declined() {
        // Non-trivial argument: the arrow would duplicate or reorder effects.
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "f".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(1, vec![param(P, "id")], Expr::LocalGet(P), false)),
            },
            Stmt::Expr(call_local(F, vec![this_exists(Expr::Integer(1))])),
        ];
        let before = stmts.clone();
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));

        // Captured by a nested closure that calls it later.
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "f".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(1, vec![], Expr::Integer(1), false)),
            },
            Stmt::Return(Some(Expr::Closure {
                func_id: 2,
                params: vec![],
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(call_local(F, vec![])))],
                captures: vec![F],
                mutable_captures: vec![],
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            })),
        ];
        let before = stmts.clone();
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }

    #[test]
    fn a_guard_on_a_reassigned_binding_stays() {
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "n".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(Expr::Integer(3)),
            },
            Stmt::Expr(Expr::LocalSet(F, Box::new(Expr::Undefined))),
            guard(F, Expr::Integer(7)),
        ];
        let before = stmts.clone();
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }

    /// A helper `c(e, t){ ... e(t) ... }` inlined as
    /// `c(() => Symbol(..))` binds `e` to a zero-parameter arrow and
    /// substitutes the missing `t` as `undefined`, so the only call is
    /// `e(undefined)`: one trivial argument for a zero-parameter arrow. The
    /// rewrite cannot reduce a call of a different arity, so the binding must
    /// be kept; deleting it left a call of an unbound local, which codegen
    /// lowered to calling the constant `undefined`.
    #[test]
    fn keeps_the_let_when_a_call_has_a_different_arity() {
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "e485".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(17, vec![], Expr::Bool(true), false)),
            },
            Stmt::Expr(call_local(F, vec![Expr::Undefined])),
        ];
        let before = stmts.clone();
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }

    /// Fewer arguments than parameters is likewise left alone.
    #[test]
    fn keeps_the_let_when_a_call_passes_fewer_arguments() {
        let mut stmts = vec![
            Stmt::Let {
                id: F,
                name: "f".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(arrow(18, vec![param(P, "x")], Expr::LocalGet(P), false)),
            },
            Stmt::Expr(call_local(F, vec![])),
        ];
        let before = stmts.clone();
        let mut next = 100;
        reduce(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }

    fn let_f(init: Expr) -> Stmt {
        Stmt::Let {
            id: F,
            name: "f".into(),
            ty: Type::Any,
            mutable: false,
            init: Some(init),
        }
    }

    /// `f = (a) => (a = 5); f(x)`: substitution renames the write onto the
    /// argument's local, so the caller's `x` would change. Its twin with a
    /// parameter that is only read is reduced.
    #[test]
    fn a_parameter_written_in_the_body_keeps_the_closure() {
        let written = arrow(
            1,
            vec![param(P, "a")],
            Expr::LocalSet(P, Box::new(Expr::Integer(5))),
            false,
        );
        let mut stmts = vec![
            let_f(written),
            Stmt::Expr(call_local(F, vec![Expr::LocalGet(ENTITY)])),
        ];
        let before = stmts.clone();
        reduce(&mut stmts, &mut 100);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));

        let read = arrow(1, vec![param(P, "a")], Expr::LocalGet(P), false);
        let mut stmts = vec![
            let_f(read),
            Stmt::Expr(call_local(F, vec![Expr::LocalGet(ENTITY)])),
        ];
        reduce(&mut stmts, &mut 100);
        assert_eq!(
            format!("{stmts:?}"),
            format!("{:?}", vec![Stmt::Expr(Expr::LocalGet(ENTITY))])
        );
    }

    /// `f(m)` where something writes `m`: the read would move to where the
    /// body reads the parameter, after any write the body makes first. A
    /// write anywhere in the module keeps the call; its unwritten twin is
    /// reduced (above).
    #[test]
    fn an_argument_local_written_anywhere_keeps_the_closure() {
        const M: LocalId = 2;
        let mut stmts = vec![
            let_f(arrow(1, vec![param(P, "a")], Expr::LocalGet(P), false)),
            Stmt::Expr(call_local(F, vec![Expr::LocalGet(M)])),
            Stmt::Expr(Expr::LocalSet(M, Box::new(Expr::Integer(2)))),
        ];
        let before = stmts.clone();
        reduce(&mut stmts, &mut 100);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }

    /// A global read is not known to be unwritten, so it is not a stable
    /// argument; a literal is.
    #[test]
    fn a_global_argument_keeps_the_closure_and_a_literal_does_not() {
        let mut stmts = vec![
            let_f(arrow(1, vec![param(P, "a")], Expr::LocalGet(P), false)),
            Stmt::Expr(call_local(F, vec![Expr::GlobalGet(7)])),
        ];
        let before = stmts.clone();
        reduce(&mut stmts, &mut 100);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));

        let mut stmts = vec![
            let_f(arrow(1, vec![param(P, "a")], Expr::LocalGet(P), false)),
            Stmt::Expr(call_local(F, vec![Expr::Integer(3)])),
        ];
        reduce(&mut stmts, &mut 100);
        assert_eq!(
            format!("{stmts:?}"),
            format!("{:?}", vec![Stmt::Expr(Expr::Integer(3))])
        );
    }
}
