//! Traversal and binding facts used by structural region admission.

use super::Limits;
use perry_hir::{Expr, Function, Module as HirModule, Stmt};
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Deep visitors
// ---------------------------------------------------------------------------

fn for_each_child_list<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a [Stmt])) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            f(then_branch);
            if let Some(e) = else_branch {
                f(e);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => f(body),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                f(std::slice::from_ref(i.as_ref()));
            }
            f(body);
        }
        Stmt::Labeled { body, .. } => f(std::slice::from_ref(body.as_ref())),
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
        Stmt::Switch { cases, .. } => {
            for c in cases {
                f(&c.body);
            }
        }
        _ => {}
    }
}

/// The expressions a statement holds directly (not those of nested lists).
fn for_each_own_expr<'a>(s: &'a Stmt, f: &mut dyn FnMut(&'a Expr)) {
    match s {
        Stmt::Let { init: Some(e), .. }
        | Stmt::Expr(e)
        | Stmt::Throw(e)
        | Stmt::Return(Some(e)) => f(e),
        Stmt::If { condition, .. }
        | Stmt::While { condition, .. }
        | Stmt::DoWhile { condition, .. } => f(condition),
        Stmt::For {
            condition, update, ..
        } => {
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
        _ => {}
    }
}

/// Visit every statement and expression under `stmts`. Closure bodies are
/// entered when `closures` is true; `on_expr` sees the closure node either way.
pub(super) fn visit_stmts<'a>(
    stmts: &'a [Stmt],
    closures: bool,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    for s in stmts {
        visit_stmt(s, closures, on_stmt, on_expr);
    }
}

pub(super) fn visit_stmt<'a>(
    s: &'a Stmt,
    closures: bool,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_stmt(s);
    for_each_own_expr(s, &mut |e| visit_expr(e, closures, on_stmt, on_expr));
    for_each_child_list(s, &mut |l| visit_stmts(l, closures, on_stmt, on_expr));
}

pub(super) fn visit_expr<'a>(
    e: &'a Expr,
    closures: bool,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    on_expr(e);
    if closures {
        if let Expr::Closure { body, .. } = e {
            visit_stmts(body, closures, on_stmt, on_expr);
        }
    }
    perry_hir::walker::walk_expr_children(e, &mut |c| visit_expr(c, closures, on_stmt, on_expr));
}

/// Every local id a statement list declares, at any depth, closures included
/// (lets, catch parameters, closure parameters, scoped temps).
pub(super) fn declared_ids(stmts: &[Stmt], closures: bool) -> HashSet<u32> {
    let mut out = HashSet::new();
    let out_cell = std::cell::RefCell::new(&mut out);
    visit_stmts(
        stmts,
        closures,
        &mut |s| match s {
            Stmt::Let { id, .. } => {
                out_cell.borrow_mut().insert(*id);
            }
            Stmt::Try { catch: Some(c), .. } => {
                if let Some((id, _)) = &c.param {
                    out_cell.borrow_mut().insert(*id);
                }
            }
            _ => {}
        },
        &mut |e| match e {
            Expr::Closure { params, .. } if closures => {
                for p in params {
                    out_cell.borrow_mut().insert(p.id);
                }
            }
            Expr::ScopedTemp { id, .. } => {
                out_cell.borrow_mut().insert(*id);
            }
            _ => {}
        },
    );
    out
}

/// Reference counts of every local id under `stmts`, closures included (a
/// closure's body reads count, as do its capture lists).
pub(super) fn ref_counts(stmts: &[Stmt], counts: &mut HashMap<u32, i64>, sign: i64) {
    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for s in stmts {
        perry_hir::collect_local_refs_stmt(s, &mut refs, &mut visited);
    }
    for id in refs {
        *counts.entry(id).or_insert(0) += sign;
    }
}

/// `Let` statements per id under `stmts` (not in nested closures).
pub(super) fn let_counts(stmts: &[Stmt]) -> HashMap<u32, usize> {
    let mut out = HashMap::new();
    visit_stmts(
        stmts,
        false,
        &mut |s| {
            if let Stmt::Let { id, .. } = s {
                *out.entry(*id).or_insert(0) += 1;
            }
        },
        &mut |_| {},
    );
    out
}

/// Turn every declaration of an id in `ids` under `stmts` (not in nested
/// closures) into an assignment: these bindings belong to the parent, so a
/// declaration with an initializer assigns it and a bare one does nothing.
pub(super) fn declarations_to_assignments(stmts: &mut Vec<Stmt>, ids: &HashMap<u32, u32>) {
    if ids.is_empty() {
        return;
    }
    let old = std::mem::take(stmts);
    for s in old {
        match s {
            Stmt::Let { id, init, .. } if ids.contains_key(&id) => {
                if let Some(e) = init {
                    stmts.push(Stmt::Expr(Expr::LocalSet(ids[&id], Box::new(e))));
                }
            }
            mut other => {
                match &mut other {
                    Stmt::If {
                        then_branch,
                        else_branch,
                        ..
                    } => {
                        declarations_to_assignments(then_branch, ids);
                        if let Some(e) = else_branch {
                            declarations_to_assignments(e, ids);
                        }
                    }
                    Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                        declarations_to_assignments(body, ids)
                    }
                    Stmt::For { init, body, .. } => {
                        if let Some(i) = init {
                            if let Stmt::Let {
                                id, init: value, ..
                            } = i.as_mut()
                            {
                                if let Some(target) = ids.get(id).copied() {
                                    let set = value
                                        .take()
                                        .map(|e| Stmt::Expr(Expr::LocalSet(target, Box::new(e))));
                                    match set {
                                        Some(st) => **i = st,
                                        None => *i = Box::new(Stmt::Expr(Expr::Undefined)),
                                    }
                                }
                            }
                        }
                        declarations_to_assignments(body, ids);
                    }
                    Stmt::Labeled { body, .. } => {
                        let mut v = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                        declarations_to_assignments(&mut v, ids);
                        **body = if v.len() <= 1 {
                            v.pop().unwrap_or(Stmt::Expr(Expr::Undefined))
                        } else {
                            Stmt::If {
                                condition: Expr::Bool(true),
                                then_branch: v,
                                else_branch: None,
                            }
                        };
                    }
                    Stmt::Try {
                        body,
                        catch,
                        finally,
                    } => {
                        declarations_to_assignments(body, ids);
                        if let Some(c) = catch {
                            declarations_to_assignments(&mut c.body, ids);
                        }
                        if let Some(f) = finally {
                            declarations_to_assignments(f, ids);
                        }
                    }
                    Stmt::Switch { cases, .. } => {
                        for c in cases {
                            declarations_to_assignments(&mut c.body, ids);
                        }
                    }
                    _ => {}
                }
                stmts.push(other);
            }
        }
    }
}

pub(super) fn ref_set(stmts: &[Stmt]) -> HashSet<u32> {
    let mut refs = Vec::new();
    let mut visited = HashSet::new();
    for s in stmts {
        perry_hir::collect_local_refs_stmt(s, &mut refs, &mut visited);
    }
    refs.into_iter().collect()
}

/// Ids written (`=`, `++`, compound assignment, `with` fallback) under
/// `stmts`, closures included.
pub(super) fn written_ids(stmts: &[Stmt], out: &mut HashSet<u32>) {
    visit_stmts(stmts, true, &mut |_| {}, &mut |e| written_expr(e, out));
}

fn written_expr(e: &Expr, out: &mut HashSet<u32>) {
    match e {
        Expr::LocalSet(id, _) | Expr::Update { id, .. } => {
            out.insert(*id);
        }
        // Codegen stores the class object into a class expression's
        // evaluation owner: a write the HIR does not spell out.
        Expr::ClassExprFresh {
            evaluation_owner: Some(id),
            ..
        } => {
            out.insert(*id);
        }
        Expr::WithSet { fallback, .. } => match fallback {
            perry_hir::WithSetFallback::Local(id)
            | perry_hir::WithSetFallback::SloppyImplicit(id) => {
                out.insert(*id);
            }
            _ => {}
        },
        _ => {}
    }
}

/// Ids referenced inside closures nested under `stmts` (their captures).
pub(super) fn closure_captured_ids(stmts: &[Stmt]) -> HashSet<u32> {
    let mut out = HashSet::new();
    let out_cell = std::cell::RefCell::new(&mut out);
    visit_stmts(stmts, true, &mut |_| {}, &mut |e| {
        if let Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } = e
        {
            let mut o = out_cell.borrow_mut();
            o.extend(captures.iter().copied());
            o.extend(mutable_captures.iter().copied());
            o.extend(ref_set(body));
        }
    });
    out
}

// ---------------------------------------------------------------------------
// Module facts
// ---------------------------------------------------------------------------

pub(super) struct ModuleFacts {
    pub(super) limits: Limits,
    /// Ids written anywhere in the module after their declaration.
    pub(super) writes: HashSet<u32>,
    /// Ids whose cell is preallocated (forward-referenced captures, TDZ).
    pub(super) prealloc: HashSet<u32>,
    /// The preallocated cells that start in the temporal dead zone: their
    /// declaration initializes the cell and must stay a declaration.
    pub(super) prealloc_tdz: HashSet<u32>,
    /// Classes whose construction reads or writes back locals through class
    /// capture parameters or a class environment.
    pub(super) capturing_classes: HashSet<String>,
    /// Locals a class expression's evaluation writes behind the HIR's back
    /// (`ClassExprFresh::evaluation_owner`): they never cross a boundary.
    pub(super) class_owners: HashSet<u32>,
}

pub(super) fn all_module_bodies(hir: &HirModule) -> Vec<&[Stmt]> {
    let mut out: Vec<&[Stmt]> = vec![&hir.init];
    for f in &hir.functions {
        out.push(&f.body);
    }
    for c in &hir.classes {
        if let Some(ctor) = &c.constructor {
            out.push(&ctor.body);
        }
        for f in c
            .methods
            .iter()
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            out.push(&f.body);
        }
    }
    out
}

/// Field initializers are expression roots, separate from class function
/// bodies. Their closures execute with the same module-wide bindings.
pub(super) fn module_field_exprs(hir: &HirModule) -> impl Iterator<Item = &Expr> {
    hir.classes.iter().flat_map(|c| {
        c.fields
            .iter()
            .chain(&c.static_fields)
            .flat_map(|f| f.key_expr.iter().chain(f.init.iter()))
    })
}

fn visit_module<'a>(
    hir: &'a HirModule,
    on_stmt: &mut dyn FnMut(&'a Stmt),
    on_expr: &mut dyn FnMut(&'a Expr),
) {
    for body in all_module_bodies(hir) {
        visit_stmts(body, true, on_stmt, on_expr);
    }
    for expr in module_field_exprs(hir) {
        visit_expr(expr, true, on_stmt, on_expr);
    }
}

pub(super) fn module_facts(hir: &HirModule, limits: Limits) -> ModuleFacts {
    let mut writes = HashSet::new();
    let mut prealloc = HashSet::new();
    let mut prealloc_tdz = HashSet::new();
    // A `var` is declared twice in HIR: a hoisted seed at the top of its
    // body and the declaration with its initializer at its source position.
    // The second declaration assigns the binding: a write.
    let mut decl_counts: HashMap<u32, usize> = HashMap::new();
    let mut class_owners = HashSet::new();
    visit_module(
        hir,
        &mut |s| match s {
            Stmt::Let { id, .. } => {
                *decl_counts.entry(*id).or_insert(0) += 1;
            }
            Stmt::PreallocateBoxes(ids) | Stmt::ReleaseBoxes(ids) => {
                prealloc.extend(ids.iter().copied());
            }
            Stmt::PreallocateTdzBoxes(ids) => {
                prealloc.extend(ids.iter().copied());
                prealloc_tdz.extend(ids.iter().copied());
            }
            _ => {}
        },
        &mut |e| {
            written_expr(e, &mut writes);
            if let Expr::ClassExprFresh {
                evaluation_owner: Some(id),
                ..
            } = e
            {
                class_owners.insert(*id);
            }
        },
    );
    writes.extend(
        decl_counts
            .iter()
            .filter(|(_, n)| **n > 1)
            .map(|(id, _)| *id),
    );
    let mut capturing_classes = HashSet::new();
    for c in &hir.classes {
        let cap_params = c.constructor.as_ref().is_some_and(|ctor| {
            ctor.params
                .iter()
                .any(|p| p.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX))
        });
        let cap_fields = c
            .fields
            .iter()
            .chain(c.static_fields.iter())
            .any(|f| f.name.starts_with(perry_hir::cap_fields::CAP_FIELD_PREFIX));
        if cap_params || cap_fields {
            capturing_classes.insert(c.name.clone());
        }
    }
    ModuleFacts {
        limits,
        writes,
        prealloc,
        prealloc_tdz,
        capturing_classes,
        class_owners,
    }
}

pub(super) fn max_local_id(hir: &HirModule) -> u32 {
    let mut max = 0u32;
    let mut note = |stmts: &[Stmt]| {
        for id in declared_ids(stmts, true) {
            max = max.max(id);
        }
        for id in ref_set(stmts) {
            max = max.max(id);
        }
        visit_stmts(
            stmts,
            true,
            &mut |s| match s {
                Stmt::PreallocateBoxes(ids)
                | Stmt::PreallocateTdzBoxes(ids)
                | Stmt::ReleaseBoxes(ids) => {
                    for id in ids {
                        max = max.max(*id);
                    }
                }
                _ => {}
            },
            &mut |_| {},
        );
    };
    for body in all_module_bodies(hir) {
        note(body);
    }
    let mut max = max;
    let mut fns: Vec<&Function> = hir.functions.iter().collect();
    for c in &hir.classes {
        fns.extend(c.constructor.iter());
        fns.extend(c.methods.iter());
        fns.extend(c.static_methods.iter());
        fns.extend(c.getters.iter().map(|(_, f)| f));
        fns.extend(c.setters.iter().map(|(_, f)| f));
        fns.extend(c.computed_members.iter().map(|m| &m.function));
    }
    for f in fns {
        for p in &f.params {
            max = max.max(p.id);
        }
        for id in &f.captures {
            max = max.max(*id);
        }
    }
    for id in hir.local_source_spans.keys() {
        max = max.max(*id);
    }
    max
}
