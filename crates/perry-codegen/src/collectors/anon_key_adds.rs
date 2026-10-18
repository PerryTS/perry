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

/// Receiver evidence, resolved to a fixed point over four edges:
///
/// * R1/R2: an unreassigned local, captured or module binding takes its
///   initializer's classes (a birth, a module-local factory, or another
///   binding);
/// * R3: `this` in a literal method is its home literal;
/// * P: an unreassigned parameter of a module-local function or closure takes
///   the classes of the argument every direct call passes in its position;
/// * E: an unreassigned array binding's elements take the classes of its
///   literal elements and of every value pushed onto it, and an indexed read
///   of it (`a[i]`, `a.at(i)`) takes its elements' classes.
///
/// A binding with several declarations fails closed. A parameter or array may
/// collect several classes; each one gets the keys. This is capacity
/// evidence only: an over-reservation costs a few inline words, an
/// under-reservation spills, and neither changes what any access may assume.
pub(crate) fn anon_receiver_added_keys(
    hir: &Module,
    classes: &HashMap<String, &Class>,
    facts: &ModuleDispatchFacts,
) -> HashMap<String, BTreeSet<String>> {
    let homes = super::literal_method_home::literal_method_home_classes(hir);
    let mut lets: HashMap<Binding, Vec<Option<&Expr>>> = HashMap::new();
    let mut writes = HashSet::new();
    let mut params: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut calls: Vec<(u32, &[Expr])> = Vec::new();
    let mut pushes: Vec<(Binding, &Expr)> = Vec::new();
    let mut note_params = |func: u32, ps: &[perry_hir::Param]| {
        params
            .entry(func)
            .or_insert_with(|| ps.iter().take_while(|p| !p.is_rest).map(|p| p.id).collect());
    };
    for f in hir.functions.iter() {
        note_params(f.id, &f.params);
    }
    visit_module(
        hir,
        &mut |stmt| {
            if let Stmt::Let { id, init, .. } = stmt {
                lets.entry((false, *id)).or_default().push(init.as_ref());
            }
        },
        &mut |expr, _| match expr {
            Expr::LocalSet(id, _) => {
                writes.insert((false, *id));
            }
            Expr::GlobalSet(id, _) => {
                writes.insert((true, *id));
            }
            Expr::Closure {
                func_id,
                params: ps,
                ..
            } => note_params(*func_id, ps),
            Expr::ScopedTemp { id, .. } => {
                writes.insert((false, *id));
            }
            Expr::Update { id, .. } => {
                writes.insert((false, *id));
            }
            Expr::Call { callee, args, .. } => {
                let func = match callee.as_ref() {
                    Expr::FuncRef(id) => Some(*id),
                    Expr::LocalGet(id) => facts.closure_binding_func(*id),
                    _ => None,
                };
                if let Some(func) = func {
                    calls.push((func, args.as_slice()));
                }
                if let (
                    Expr::PropertyGet {
                        object, property, ..
                    },
                    [value],
                ) = (callee.as_ref(), args.as_slice())
                {
                    if property == "push" {
                        if let Some(b) = binding_of(object) {
                            pushes.push((b, value));
                        }
                    }
                }
            }
            Expr::ArrayPush {
                array_id, value, ..
            } => pushes.push(((false, *array_id), value.as_ref())),
            _ => {}
        },
    );
    for global in &hir.globals {
        lets.entry((true, global.id))
            .or_default()
            .push(global.init.as_ref());
    }

    let mut receivers: HashMap<Binding, BTreeSet<String>> = HashMap::new();
    let mut elements: HashMap<Binding, BTreeSet<String>> = HashMap::new();
    // Each round can only add classes, and the class universe is finite, so
    // this settles; the bound keeps a pathological chain from walking long.
    for _ in 0..16 {
        let mut changed = false;
        for (b, inits) in &lets {
            let [Some(init)] = inits.as_slice() else {
                continue;
            };
            if writes.contains(b) {
                continue;
            }
            let found = expr_classes(init, facts, &receivers, &elements);
            changed |= merge(&mut receivers, *b, found);
            if let Expr::Array(items, _) = init {
                for item in items {
                    let found = expr_classes(item, facts, &receivers, &elements);
                    changed |= merge(&mut elements, *b, found);
                }
            }
        }
        for (func, args) in &calls {
            let Some(ps) = params.get(func) else {
                continue;
            };
            for (param, arg) in ps.iter().zip(args.iter()) {
                if writes.contains(&(false, *param)) {
                    continue;
                }
                let found = expr_classes(arg, facts, &receivers, &elements);
                changed |= merge(&mut receivers, (false, *param), found);
            }
        }
        for (b, value) in &pushes {
            if writes.contains(b) {
                continue;
            }
            let found = expr_classes(value, facts, &receivers, &elements);
            changed |= merge(&mut elements, *b, found);
        }
        if !changed {
            break;
        }
    }

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
        let first = key.as_bytes().first().copied().unwrap_or(b'0');
        if first == b'#' || first.is_ascii_digit() {
            return;
        }
        let mut names: Vec<&str> = Vec::new();
        match receiver {
            Expr::This => names.extend(home.and_then(|id| homes.get(&id)).map(String::as_str)),
            other => {
                if let Some(set) = binding_of(other).and_then(|b| receivers.get(&b)) {
                    names.extend(set.iter().map(String::as_str));
                } else if let Some(set) = indexed_array(other).and_then(|b| elements.get(&b)) {
                    names.extend(set.iter().map(String::as_str));
                }
            }
        }
        for name in names {
            let Some(class) = classes.get(name) else {
                continue;
            };
            if class.is_literal_shape() && !class.fields.iter().any(|field| field.name == key) {
                added
                    .entry(name.to_owned())
                    .or_default()
                    .insert(key.to_owned());
            }
        }
    });
    added
}

/// Add `from` to `into[b]`; true when that grew the set.
fn merge(
    into: &mut HashMap<Binding, BTreeSet<String>>,
    b: Binding,
    from: BTreeSet<String>,
) -> bool {
    if from.is_empty() {
        return false;
    }
    let set = into.entry(b).or_default();
    let before = set.len();
    set.extend(from);
    set.len() != before
}

fn binding_of(expr: &Expr) -> Option<Binding> {
    match expr {
        Expr::LocalGet(id) => Some((false, *id)),
        Expr::GlobalGet(id) => Some((true, *id)),
        _ => None,
    }
}

/// The array binding an element read `a[i]` / `a.at(i)` reads from.
fn indexed_array(expr: &Expr) -> Option<Binding> {
    match expr {
        Expr::IndexGet { object, .. } => binding_of(object),
        Expr::ArrayAt { array, .. } => binding_of(array),
        _ => None,
    }
}

/// The literal classes `expr` may evaluate to, from the evidence so far.
fn expr_classes(
    expr: &Expr,
    facts: &ModuleDispatchFacts,
    receivers: &HashMap<Binding, BTreeSet<String>>,
    elements: &HashMap<Binding, BTreeSet<String>>,
) -> BTreeSet<String> {
    if let Some(class) = seed(expr, facts) {
        return BTreeSet::from([class.to_owned()]);
    }
    binding_of(expr)
        .and_then(|b| receivers.get(&b))
        .or_else(|| indexed_array(expr).and_then(|b| elements.get(&b)))
        .cloned()
        .unwrap_or_default()
}

// One exhaustive statement walk supplies declarations and expressions. The
// expression callback carries the owning closure id; ordinary functions have
// no literal home. Arrow closures inherit their enclosing `this`.
fn visit_module<'a>(
    hir: &'a Module,
    stmt: &mut dyn FnMut(&'a Stmt),
    expr: &mut dyn FnMut(&'a Expr, Option<u32>),
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

fn visit_expr<'a>(
    e: &'a Expr,
    home: Option<u32>,
    stmt: &mut dyn FnMut(&'a Stmt),
    expr: &mut dyn FnMut(&'a Expr, Option<u32>),
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

fn visit_stmts<'a>(
    body: &'a [Stmt],
    home: Option<u32>,
    stmt: &mut dyn FnMut(&'a Stmt),
    expr: &mut dyn FnMut(&'a Expr, Option<u32>),
) {
    for s in body {
        visit_stmt(s, home, stmt, expr);
    }
}

fn visit_stmt<'a>(
    s: &'a Stmt,
    home: Option<u32>,
    stmt: &mut dyn FnMut(&'a Stmt),
    expr: &mut dyn FnMut(&'a Expr, Option<u32>),
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
