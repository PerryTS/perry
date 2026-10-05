//! Capacity evidence for literal births. This is deliberately independent of
//! containment: escaping a record does not make reserving space unsafe. Keys
//! remain authoritative and no access is licensed by these facts.

use std::collections::{BTreeSet, HashMap, HashSet};

use perry_hir::{Class, Expr, Module, Stmt};

use super::ModuleDispatchFacts;

type Binding = (bool, u32); // global/local namespaces are distinct

fn seed<'a>(expr: &'a Expr, facts: &'a ModuleDispatchFacts) -> Option<&'a str> {
    match expr {
        Expr::New { class_name, .. } => Some(class_name),
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::FuncRef(id) => facts.return_shape_class(*id),
            Expr::LocalGet(id) => facts
                .closure_binding_func(*id)
                .and_then(|id| facts.return_shape_class(id)),
            // Imported births belong to the producer module. Reserving only
            // a consumer's same-named class would not widen those births.
            _ => None,
        },
        _ => None,
    }
}

/// R1/R2 bindings (including captures and module bindings), and R3 literal
/// method `this`. Parameters and array elements intentionally contribute no
/// evidence in phase 1. Duplicate binding ids and reassignment fail closed.
pub(crate) fn anon_receiver_added_keys(
    hir: &Module,
    classes: &HashMap<String, &Class>,
    facts: &ModuleDispatchFacts,
) -> HashMap<String, BTreeSet<String>> {
    let homes = super::literal_method_home::literal_method_home_classes(hir);
    let mut bindings: HashMap<Binding, Vec<Option<String>>> = HashMap::new();
    let mut writes = HashSet::new();
    visit_module(
        hir,
        &mut |stmt| {
            if let Stmt::Let { id, init, .. } = stmt {
                bindings.entry((false, *id)).or_default().push(
                    init.as_ref()
                        .and_then(|e| seed(e, facts))
                        .map(str::to_owned),
                );
            }
        },
        &mut |expr, _| match expr {
            Expr::LocalSet(id, _) => {
                writes.insert((false, *id));
            }
            Expr::GlobalSet(id, _) => {
                writes.insert((true, *id));
            }
            Expr::Closure { params, .. } => {
                writes.extend(params.iter().map(|param| (false, param.id)));
            }
            Expr::ScopedTemp { id, .. } => {
                writes.insert((false, *id));
            }
            Expr::Update { id, .. } => {
                writes.insert((false, *id));
            }
            _ => {}
        },
    );
    for f in hir.functions.iter().chain(hir.classes.iter().flat_map(|c| {
        c.constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
    })) {
        writes.extend(f.params.iter().map(|param| (false, param.id)));
    }
    for global in &hir.globals {
        bindings.entry((true, global.id)).or_default().push(
            global
                .init
                .as_ref()
                .and_then(|e| seed(e, facts))
                .map(str::to_owned),
        );
    }
    let bindings: HashMap<_, _> = bindings
        .into_iter()
        .filter_map(|(id, seeds)| {
            if writes.contains(&id) || seeds.len() != 1 {
                return None;
            }
            seeds.into_iter().next().flatten().map(|class| (id, class))
        })
        .collect();
    let mut added: HashMap<String, BTreeSet<String>> = HashMap::new();
    visit_module(hir, &mut |_| {}, &mut |expr, home| {
        let (receiver, key) = match expr {
            Expr::PropertySet {
                object, property, ..
            } => (object.as_ref(), property.as_str()),
            Expr::PutValueSet { target, key, .. } => {
                let Expr::String(key) = key.as_ref() else {
                    return;
                };
                (target.as_ref(), key.as_str())
            }
            _ => return,
        };
        let class_name = match receiver {
            Expr::LocalGet(id) => bindings.get(&(false, *id)).map(String::as_str),
            Expr::GlobalGet(id) => bindings.get(&(true, *id)).map(String::as_str),
            Expr::This => home.and_then(|id| homes.get(&id)).map(String::as_str),
            _ => None,
        };
        let Some(name) = class_name else {
            return;
        };
        let Some(class) = classes.get(name) else {
            return;
        };
        let first = key.as_bytes().first().copied().unwrap_or(b'0');
        if class.is_literal_shape()
            && first != b'#'
            && !first.is_ascii_digit()
            && !class.fields.iter().any(|field| field.name == key)
        {
            added
                .entry(name.to_owned())
                .or_default()
                .insert(key.to_owned());
        }
    });
    added
}

// One exhaustive statement walk supplies declarations and expressions. The
// expression callback carries the owning closure id; ordinary functions have
// no literal home. Arrow closures inherit their enclosing `this`.
fn visit_module(
    hir: &Module,
    stmt: &mut dyn FnMut(&Stmt),
    expr: &mut dyn FnMut(&Expr, Option<u32>),
) {
    visit_stmts(&hir.init, None, stmt, expr);
    for f in &hir.functions {
        visit_stmts(&f.body, None, stmt, expr);
    }
    for c in &hir.classes {
        for f in c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.static_methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.computed_members.iter().map(|m| &m.function))
        {
            visit_stmts(&f.body, None, stmt, expr);
        }
        for field in c.fields.iter().chain(c.static_fields.iter()) {
            for e in field.init.iter().chain(field.key_expr.iter()) {
                visit_expr(e, None, stmt, expr);
            }
        }
    }
    for g in &hir.globals {
        if let Some(e) = &g.init {
            visit_expr(e, None, stmt, expr);
        }
    }
}

fn visit_expr(
    e: &Expr,
    home: Option<u32>,
    stmt: &mut dyn FnMut(&Stmt),
    expr: &mut dyn FnMut(&Expr, Option<u32>),
) {
    expr(e, home);
    perry_hir::walker::walk_expr_children(e, &mut |e| visit_expr(e, home, stmt, expr));
    if let Expr::Closure {
        func_id,
        body,
        is_arrow,
        captures_this,
        ..
    } = e
    {
        let home = if *is_arrow && *captures_this {
            home
        } else {
            Some(*func_id)
        };
        visit_stmts(body, home, stmt, expr);
    }
}

fn visit_stmts(
    body: &[Stmt],
    home: Option<u32>,
    stmt: &mut dyn FnMut(&Stmt),
    expr: &mut dyn FnMut(&Expr, Option<u32>),
) {
    for s in body {
        visit_stmt(s, home, stmt, expr);
    }
}

fn visit_stmt(
    s: &Stmt,
    home: Option<u32>,
    stmt: &mut dyn FnMut(&Stmt),
    expr: &mut dyn FnMut(&Expr, Option<u32>),
) {
    stmt(s);
    match s {
        Stmt::Let { init, .. } | Stmt::Return(init) => {
            if let Some(e) = init {
                visit_expr(e, home, stmt, expr);
            }
        }
        Stmt::Expr(e) | Stmt::Throw(e) => visit_expr(e, home, stmt, expr),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, home, stmt, expr);
            visit_stmts(then_branch, home, stmt, expr);
            if let Some(body) = else_branch {
                visit_stmts(body, home, stmt, expr);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { condition, body } => {
            visit_expr(condition, home, stmt, expr);
            visit_stmts(body, home, stmt, expr);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(s) = init {
                visit_stmt(s, home, stmt, expr);
            }
            for e in condition.iter().chain(update.iter()) {
                visit_expr(e, home, stmt, expr);
            }
            visit_stmts(body, home, stmt, expr);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmts(body, home, stmt, expr);
            if let Some(c) = catch {
                visit_stmts(&c.body, home, stmt, expr);
            }
            if let Some(body) = finally {
                visit_stmts(body, home, stmt, expr);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, home, stmt, expr);
            for c in cases {
                if let Some(e) = &c.test {
                    visit_expr(e, home, stmt, expr);
                }
                visit_stmts(&c.body, home, stmt, expr);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, home, stmt, expr),
        Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_)
        | Stmt::ReleaseBoxes(_) => {}
    }
}
