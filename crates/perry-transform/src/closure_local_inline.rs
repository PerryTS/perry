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
//!    only plain parameters, no mutable captures, no `new.target` capture,
//!    and a single `return <expr>` body, and where every use of `f` in the
//!    rest of the list is a direct call with exactly the declared number of
//!    trivial arguments (locals, globals, literals — evaluated once either
//!    way, so substitution preserves order and count of effects);
//! 3. replaces each such call with the arrow's return expression, parameters
//!    substituted by the arguments, and deletes the `let`.
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

pub fn run(module: &mut Module) {
    let mut next_local_id = crate::generator::compute_max_local_id(module).saturating_add(1);
    for f in &mut module.functions {
        run_function(f, &mut next_local_id);
    }
    for c in &mut module.classes {
        if let Some(ctor) = &mut c.constructor {
            run_function(ctor, &mut next_local_id);
        }
        for m in &mut c.methods {
            run_function(m, &mut next_local_id);
        }
        for m in &mut c.static_methods {
            run_function(m, &mut next_local_id);
        }
        for (_, g) in &mut c.getters {
            run_function(g, &mut next_local_id);
        }
        for (_, s) in &mut c.setters {
            run_function(s, &mut next_local_id);
        }
    }
}

fn run_function(f: &mut Function, next_local_id: &mut LocalId) {
    // The async/generator transforms rewrite these bodies into state machines
    // whose locals are boxed cells; keep the shapes they expect.
    if f.is_async || f.is_generator {
        return;
    }
    process_stmts(&mut f.body, next_local_id);
}

fn process_stmts(stmts: &mut Vec<Stmt>, next_local_id: &mut LocalId) {
    // Inner lists and closure bodies first, so a nested helper is reduced in
    // its own scope before the enclosing list is examined.
    for s in stmts.iter_mut() {
        for inner in nested_stmt_lists(s) {
            process_stmts(inner, next_local_id);
        }
        for_each_expr_in_stmt_mut(s, &mut |e| process_closure_bodies(e, next_local_id));
    }

    remove_dead_default_guards(stmts);

    let mut i = 0;
    while i < stmts.len() {
        let candidate = match &stmts[i] {
            Stmt::Let {
                id,
                init: Some(init),
                ..
            } => arrow_candidate(*id, init),
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
        let mut uses = Uses::default();
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

fn process_closure_bodies(expr: &mut Expr, next_local_id: &mut LocalId) {
    if let Expr::Closure { body, .. } = expr {
        process_stmts(body, next_local_id);
    }
    walk_expr_children_mut(expr, &mut |child| {
        process_closure_bodies(child, next_local_id)
    });
}

/// `let f = (a, b) => <expr>` with the admission rules from the module doc.
/// Returns the binding, its parameter ids, and the return expression.
fn arrow_candidate(id: LocalId, init: &Expr) -> Option<(LocalId, Vec<LocalId>, Expr)> {
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
    {
        return None;
    }
    let [Stmt::Return(Some(expr))] = body.as_slice() else {
        return None;
    };
    Some((id, params.iter().map(|p| p.id).collect(), expr.clone()))
}

/// An argument that substitution may duplicate or drop without changing
/// the program's effects: a binding read or a literal.
fn is_trivial_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Integer(_)
            | Expr::Number(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Null
            | Expr::Undefined
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
    )
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

#[derive(Default)]
struct Uses {
    calls: usize,
    bad_arity: bool,
    other: bool,
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
            if !args.iter().all(is_trivial_expr) {
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
            callee: Box::new(Expr::LocalGet(id)),
            args,
            type_args: Vec::new(),
            byte_offset: 0,
        }
    }

    fn this_exists(arg: Expr) -> Expr {
        Expr::Call {
            callee: Box::new(Expr::PropertyGet {
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
        process_stmts(&mut stmts, &mut next);
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
        process_stmts(&mut stmts, &mut next);
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
        process_stmts(&mut stmts, &mut next);
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
                callee: Box::new(Expr::GlobalGet(0)),
                args: vec![Expr::LocalGet(F)],
                type_args: Vec::new(),
                byte_offset: 0,
            }),
            Stmt::Expr(call_local(F, vec![Expr::Integer(1)])),
        ];
        let before = stmts.clone();
        let mut next = 100;
        process_stmts(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
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
        process_stmts(&mut stmts, &mut next);
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
        process_stmts(&mut stmts, &mut next);
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
        process_stmts(&mut stmts, &mut next);
        assert_eq!(format!("{stmts:?}"), format!("{before:?}"));
    }
}
