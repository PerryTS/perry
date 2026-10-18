//! Expose singleton prototype births behind run-once factory calls.
//!
//! This is bounded beta reduction, never a change to prototype identity.
//! Each call retains its own parameter bindings, object births and freshly
//! numbered closure bodies. Only module-init statements are expanded; loop,
//! callback and exported-function invocations keep the ordinary call path.
//! The driver supplies AWB2's whole-program loader-capability proof.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use perry_hir::types::{FuncId, LocalId};
use perry_hir::walker::{stmt_any_expr, walk_expr_children, walk_expr_children_mut};
use perry_hir::{Expr, Module, Param, Stmt};

use crate::closure_local_inline::{for_each_expr_in_stmt_mut, nested_stmt_lists};
use crate::inline::{collect_declared_local_ids, substitute_locals_in_stmts};

#[derive(Clone)]
struct Factory {
    params: Vec<Param>,
    body: Vec<Stmt>,
}

#[derive(Default, Debug)]
pub struct PrototypeFactoryReport {
    pub expanded: usize,
    pub refused: BTreeMap<&'static str, usize>,
}

impl PrototypeFactoryReport {
    fn refuse(&mut self, reason: &'static str) {
        *self.refused.entry(reason).or_default() += 1;
    }
}

fn prototype_factory(params: &[Param], body: &[Stmt], plain: bool) -> Option<Factory> {
    if !plain
        || params
            .iter()
            .any(|p| p.default.is_some() || p.is_rest || p.arguments_object.is_some())
        || body.is_empty()
        || body.len() > 16
        || !body[..body.len() - 1].iter().all(|s| {
            matches!(
                s,
                Stmt::Let {
                    mutable: false,
                    init: Some(_),
                    ..
                }
            )
        })
    {
        return None;
    }
    let Stmt::Return(Some(result)) = body.last()? else {
        return None;
    };
    // Closed object results are also useful when the caller uses the result
    // as a prototype. Closure factories must actually capture a prototype.
    let object = matches!(
        result,
        Expr::Object(_) | Expr::ObjectSpread { .. } | Expr::New { .. } | Expr::ObjectCreate(_, _)
    );
    let mut creates = false;
    for s in body {
        stmt_any_expr(s, &mut |e| {
            inspect_expr(e, &mut |e| creates |= matches!(e, Expr::ObjectCreate(_, _)));
            false
        });
    }
    if !object && !(matches!(result, Expr::Closure { .. }) && creates) {
        return None;
    }
    // A factory invocation's own this/arguments/new.target cannot become
    // module-init bindings. Nested ordinary function bodies keep their scope.
    fn forbidden(e: &Expr) -> bool {
        if let Expr::Closure {
            is_arrow: false, ..
        } = e
        {
            return false;
        }
        if matches!(e, Expr::This | Expr::NewTarget) {
            return true;
        }
        let mut bad = false;
        if let Expr::Closure {
            body,
            captures_this,
            captures_new_target,
            ..
        } = e
        {
            bad |= *captures_this || *captures_new_target;
            for s in body {
                bad |= stmt_any_expr(s, &mut forbidden);
            }
        }
        walk_expr_children(e, &mut |child| bad |= forbidden(child));
        bad
    }
    if body.iter().any(|s| stmt_any_expr(s, &mut forbidden)) {
        return None;
    }
    Some(Factory {
        params: params.to_vec(),
        body: body.to_vec(),
    })
}

fn inspect_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    if let Expr::Closure { body, .. } = e {
        for s in body {
            stmt_any_expr(s, &mut |e| {
                inspect_expr(e, f);
                false
            });
        }
    }
    walk_expr_children(e, &mut |child| inspect_expr(child, f));
}

fn visit_stmts_mut(body: &mut [Stmt], f: &mut impl FnMut(&mut Expr)) {
    fn visit(e: &mut Expr, f: &mut impl FnMut(&mut Expr)) {
        f(e);
        if let Expr::Closure { body, .. } = e {
            visit_stmts_mut(body, f);
        }
        walk_expr_children_mut(e, &mut |child| visit(child, f));
    }
    for s in body {
        for_each_expr_in_stmt_mut(s, &mut |e| visit(e, f));
        for inner in nested_stmt_lists(s) {
            visit_stmts_mut(inner, f);
        }
    }
}

/// Run immediately before the final const fold, after cross-module harvests.
/// No new runtime records, lookup tables or validation state are introduced.
pub fn specialize_prototype_factories(module: &mut Module, closed: bool) -> PrototypeFactoryReport {
    let mut report = PrototypeFactoryReport::default();
    let mut funcs = HashMap::new();
    for f in &module.functions {
        if let Some(candidate) = prototype_factory(
            &f.params,
            &f.body,
            !f.is_async && !f.is_generator && f.captures.is_empty(),
        ) {
            funcs.insert(f.id, candidate);
        }
    }
    let mut local_factories = HashMap::new();
    let mut prototypes = HashSet::new();
    perry_hir::for_each_module_expr(module, &mut |e| {
        if let Expr::ObjectCreate(p, _) = e {
            if let Expr::LocalGet(id) = p.as_ref() {
                prototypes.insert(*id);
            }
        }
    });
    let mut written = HashSet::new();
    perry_hir::for_each_module_expr(module, &mut |e| {
        if let Expr::LocalSet(id, _) | Expr::Update { id, .. } = e {
            written.insert(*id);
        }
    });
    let mut next_local = crate::generator::compute_max_local_id(module).saturating_add(1);
    let mut next_func = crate::generator::compute_max_func_id(module).saturating_add(1);
    let remaps = crate::source_spans::RemapSession::start(next_local);
    let mut init = std::mem::take(&mut module.init);
    let mut i = 0;
    while i < init.len() {
        let Stmt::Let {
            id,
            name,
            ty,
            mutable: false,
            init: Some(expr),
        } = &init[i]
        else {
            i += 1;
            continue;
        };
        if let Expr::Closure {
            params,
            body,
            mutable_captures,
            captures_this,
            captures_new_target,
            is_async,
            is_generator,
            ..
        } = expr
        {
            if !written.contains(id) {
                if let Some(f) = prototype_factory(
                    params,
                    body,
                    mutable_captures.is_empty()
                        && !*captures_this
                        && !*captures_new_target
                        && !*is_async
                        && !*is_generator,
                ) {
                    local_factories.insert(*id, f);
                }
            }
        }
        let Expr::Call { callee, args, .. } = expr else {
            i += 1;
            continue;
        };
        let factory = match callee.as_ref() {
            Expr::FuncRef(id) => funcs.get(id),
            Expr::LocalGet(id) => local_factories.get(id),
            _ => None,
        };
        let Some(factory) = factory else {
            i += 1;
            continue;
        };
        if !closed {
            report.refuse("module-capability");
            i += 1;
            continue;
        }
        if report.expanded == 128 {
            report.refuse("expansion-budget");
            i += 1;
            continue;
        }
        if args.len() != factory.params.len() {
            report.refuse("arity");
            i += 1;
            continue;
        }
        let Stmt::Return(Some(result)) = factory.body.last().unwrap() else {
            unreachable!()
        };
        if !matches!(result, Expr::Closure { .. }) && !prototypes.contains(id) {
            i += 1;
            continue;
        }
        let mut body = factory.body.clone();
        let mut bindings: HashSet<LocalId> = factory.params.iter().map(|p| p.id).collect();
        collect_declared_local_ids(&body, &mut bindings);
        // Parameters and declarations of nested closures need fresh locals,
        // too: codegen compiles each cloned closure under its new function id.
        visit_stmts_mut(&mut body, &mut |e| {
            if let Expr::Closure { params, body, .. } = e {
                bindings.extend(params.iter().map(|p| p.id));
                collect_declared_local_ids(body, &mut bindings);
            }
        });
        let bindings: BTreeSet<LocalId> = bindings.into_iter().collect();
        let mut map = HashMap::new();
        for old in bindings {
            let fresh = next_local;
            next_local += 1;
            map.insert(old, Expr::LocalGet(fresh));
        }
        substitute_locals_in_stmts(&mut body, &map, &mut next_local);
        // substitute_locals remaps uses and capture lists, but closure formal
        // parameter declarations belong to the body ABI and are explicit.
        let mut closure_ids = HashMap::<FuncId, FuncId>::new();
        visit_stmts_mut(&mut body, &mut |e| {
            if let Expr::Closure {
                func_id, params, ..
            } = e
            {
                let old = *func_id;
                let fresh = *closure_ids.entry(old).or_insert_with(|| {
                    let id = next_func;
                    next_func += 1;
                    id
                });
                *func_id = fresh;
                for p in params {
                    if let Some(Expr::LocalGet(new)) = map.get(&p.id) {
                        p.id = *new;
                    }
                }
                if let Some(v) = module.closure_display_names.get(&old).cloned() {
                    module.closure_display_names.insert(fresh, v);
                }
                if let Some(v) = module.closure_source_text.get(&old).cloned() {
                    module.closure_source_text.insert(fresh, v);
                }
            }
        });
        let mut expanded = Vec::new();
        for (p, arg) in factory.params.iter().zip(args) {
            let Expr::LocalGet(fresh) = map[&p.id] else {
                unreachable!()
            };
            expanded.push(Stmt::Let {
                id: fresh,
                name: p.name.clone(),
                ty: p.ty.clone(),
                mutable: false,
                init: Some(arg.clone()),
            });
        }
        let Stmt::Return(Some(result)) = body.pop().unwrap() else {
            unreachable!()
        };
        expanded.extend(body);
        expanded.push(Stmt::Let {
            id: *id,
            name: name.clone(),
            ty: ty.clone(),
            mutable: false,
            init: Some(result),
        });
        init.splice(i..=i, expanded);
        report.expanded += 1;
        // Revisit the inserted prefix: an immutable local can itself be a
        // prototype factory call. The global expansion budget bounds nesting.
    }
    module.init = init;
    remaps.finish(&mut module.local_source_spans);
    if std::env::var_os("PERRY_LAYOUT_REPORT").is_some()
        && (report.expanded != 0 || !report.refused.is_empty())
    {
        eprintln!(
            "perry: prototype factories: {} expanded, refused {:?}: {}",
            report.expanded, report.refused, module.name
        );
    }
    report
}

#[cfg(test)]
#[path = "prototype_factories_tests.rs"]
mod tests;
