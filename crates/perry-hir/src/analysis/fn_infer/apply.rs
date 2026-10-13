//! fninfer's consumer step (`PERRY_FNINFER_APPLY`): inferred parameter
//! types written into the HIR as guarded annotations.

use std::collections::HashSet;

use super::ModuleInference;
use crate::ir::*;
use crate::types::{FuncId, Type};

/// Which inferred parameter types become the parameter's HIR type
/// (`PERRY_FNINFER_APPLY`). Codegen treats a parameter type as a candidate
/// that its entry guards prove, exactly like a TypeScript annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyPolicy {
    /// Number, string and boolean parameters only.
    Scalars,
    /// Scalars plus ES-class instances.
    ScalarsAndClasses,
}

fn sabotage() -> bool {
    std::env::var("PERRY_FNINFER_SABOTAGE").is_ok_and(|v| v == "1")
}

fn applicable(t: &Type, policy: ApplyPolicy, es_classes: &HashSet<String>) -> Option<Type> {
    match t {
        Type::Int32 | Type::Number => Some(Type::Number),
        Type::String | Type::StringLiteral(_) => Some(Type::String),
        Type::Boolean => Some(Type::Boolean),
        Type::Named(c) if policy == ApplyPolicy::ScalarsAndClasses && es_classes.contains(c) => {
            Some(t.clone())
        }
        _ => None,
    }
}

/// A receiver type ([`receiver_params`]): a class, or a union of up to four
/// classes.
fn applicable_receiver(t: &Type, es_classes: &HashSet<String>) -> Option<Type> {
    let class = |t: &Type| matches!(t, Type::Named(c) if es_classes.contains(c));
    match t {
        Type::Named(_) if class(t) => Some(t.clone()),
        Type::Union(v) if (2..=4).contains(&v.len()) && v.iter().all(class) => Some(t.clone()),
        _ => None,
    }
}

/// Write inferred parameter types into the HIR where the declared type is
/// `Any`/`Unknown`. Returns the number of parameters typed.
pub fn apply_param_types(m: &mut Module, inf: &ModuleInference, policy: ApplyPolicy) -> usize {
    let es_classes: HashSet<String> = m
        .classes
        .iter()
        .filter(|c| !c.name.starts_with("__AnonShape_"))
        .map(|c| c.name.clone())
        .collect();
    let mut n = 0usize;
    let mut fix = |id: FuncId, params: &mut [Param]| {
        let Some(sig) = inf.sigs.get(&id) else { return };
        for (p, t) in params.iter_mut().zip(sig.params.iter()) {
            if p.is_rest || !matches!(p.ty, Type::Any | Type::Unknown) {
                continue;
            }
            if let Some(t) = applicable(t, policy, &es_classes) {
                // Negative control (`PERRY_FNINFER_SABOTAGE=1`): plant a wrong
                // fact. The entry guard must route every call to the generic
                // body, so outputs stay correct.
                p.ty = if sabotage() && t == Type::Number {
                    Type::String
                } else {
                    t
                };
                n += 1;
            }
        }
    };
    let mut init = std::mem::take(&mut m.init);
    apply_stmts(&mut init, &mut fix);
    m.init = init;
    for f in m.functions.iter_mut() {
        fix(f.id, &mut f.params);
        apply_stmts(&mut f.body, &mut fix);
    }
    for c in m.classes.iter_mut() {
        if c.name.starts_with("__AnonShape_") {
            continue;
        }
        let fns = c
            .constructor
            .iter_mut()
            .chain(c.methods.iter_mut())
            .chain(c.getters.iter_mut().map(|(_, f)| f))
            .chain(c.setters.iter_mut().map(|(_, f)| f))
            .chain(c.static_methods.iter_mut());
        for f in fns {
            fix(f.id, &mut f.params);
            apply_stmts(&mut f.body, &mut fix);
        }
        for fld in c.fields.iter_mut().chain(c.static_fields.iter_mut()) {
            if let Some(e) = fld.init.as_mut() {
                apply_expr(e, &mut fix);
            }
        }
    }
    n
}

fn apply_stmts(body: &mut [Stmt], fix: &mut dyn FnMut(FuncId, &mut [Param])) {
    for s in body.iter_mut() {
        match s {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    apply_expr(e, fix)
                }
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    apply_expr(e, fix)
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => apply_expr(e, fix),
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                apply_expr(condition, fix);
                apply_stmts(then_branch, fix);
                if let Some(b) = else_branch {
                    apply_stmts(b, fix);
                }
            }
            Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
                apply_expr(condition, fix);
                apply_stmts(body, fix);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    apply_stmts(std::slice::from_mut(i.as_mut()), fix);
                }
                if let Some(c) = condition {
                    apply_expr(c, fix);
                }
                if let Some(u) = update {
                    apply_expr(u, fix);
                }
                apply_stmts(body, fix);
            }
            Stmt::Labeled { body, .. } => apply_stmts(std::slice::from_mut(body.as_mut()), fix),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                apply_stmts(body, fix);
                if let Some(c) = catch {
                    apply_stmts(&mut c.body, fix);
                }
                if let Some(b) = finally {
                    apply_stmts(b, fix);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                apply_expr(discriminant, fix);
                for c in cases.iter_mut() {
                    if let Some(t) = c.test.as_mut() {
                        apply_expr(t, fix);
                    }
                    apply_stmts(&mut c.body, fix);
                }
            }
            Stmt::Break
            | Stmt::Continue
            | Stmt::LabeledBreak(_)
            | Stmt::LabeledContinue(_)
            | Stmt::PreallocateBoxes(_)
            | Stmt::PreallocateTdzBoxes(_)
            | Stmt::ReleaseBoxes(_) => {}
        }
    }
}

fn apply_expr(e: &mut Expr, fix: &mut dyn FnMut(FuncId, &mut [Param])) {
    if let Expr::Closure {
        func_id,
        params,
        body,
        ..
    } = e
    {
        fix(*func_id, params);
        for p in params.iter_mut() {
            if let Some(d) = p.default.as_mut() {
                apply_expr(d, fix);
            }
        }
        apply_stmts(body, fix);
        return;
    }
    crate::walker::walk_expr_children_mut(e, &mut |c| apply_expr(c, fix));
}

/// Step 5b: the closure parameters (by `LocalId`) declared `any` whose
/// inferred type is an ES class or a union of up to four ES classes, with the
/// class names. Codegen's typed bodies entry-check them and carry them to
/// field sites as receiver proofs; the HIR is not changed.
pub fn receiver_params(
    m: &Module,
    inf: &ModuleInference,
) -> std::collections::BTreeMap<u32, Vec<String>> {
    let es_classes: HashSet<String> = m
        .classes
        .iter()
        .filter(|c| !c.name.starts_with("__AnonShape_"))
        .map(|c| c.name.clone())
        .collect();
    let mut out = std::collections::BTreeMap::new();
    let mut found = |id: FuncId, params: &[Param]| {
        let Some(sig) = inf.sigs.get(&id) else { return };
        for (p, t) in params.iter().zip(sig.params.iter()) {
            if p.is_rest || !matches!(p.ty, Type::Any | Type::Unknown) {
                continue;
            }
            let names = match applicable_receiver(t, &es_classes) {
                Some(Type::Named(c)) => vec![c],
                Some(Type::Union(v)) => v
                    .into_iter()
                    .filter_map(|t| match t {
                        Type::Named(c) => Some(c),
                        _ => None,
                    })
                    .collect(),
                _ => continue,
            };
            out.insert(p.id, names);
        }
    };
    visit_stmts(&m.init, &mut found);
    for f in &m.functions {
        visit_stmts(&f.body, &mut found);
    }
    for c in &m.classes {
        let fns = c
            .constructor
            .iter()
            .chain(c.methods.iter())
            .chain(c.getters.iter().map(|(_, f)| f))
            .chain(c.setters.iter().map(|(_, f)| f))
            .chain(c.static_methods.iter());
        for f in fns {
            visit_stmts(&f.body, &mut found);
        }
        for fld in c.fields.iter().chain(c.static_fields.iter()) {
            if let Some(e) = fld.init.as_ref() {
                visit_expr(e, &mut found);
            }
        }
    }
    out
}

fn visit_stmts(body: &[Stmt], found: &mut dyn FnMut(FuncId, &[Param])) {
    for s in body {
        crate::walker::stmt_any_expr(s, &mut |e| {
            visit_expr(e, found);
            false
        });
    }
}

fn visit_expr(e: &Expr, found: &mut dyn FnMut(FuncId, &[Param])) {
    if let Expr::Closure {
        func_id,
        params,
        body,
        ..
    } = e
    {
        found(*func_id, params);
        for p in params {
            if let Some(d) = p.default.as_ref() {
                visit_expr(d, found);
            }
        }
        visit_stmts(body, found);
        return;
    }
    crate::walker::walk_expr_children(e, &mut |c| visit_expr(c, found));
}
