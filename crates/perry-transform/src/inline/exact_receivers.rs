use perry_hir::types::LocalId;
use perry_hir::walker::walk_expr_children;
use perry_hir::{Expr, Stmt};
use std::collections::{HashMap, HashSet};

use super::*;

// #854: receiver-class resolver for the exact-receiver inliner; retained as a
// pub helper of this pass, not wired into a call site on the current path.
#[allow(dead_code)]
pub fn resolve_receiver_class(
    obj: &Expr,
    local_types: &HashMap<LocalId, String>,
    enclosing_class: Option<&str>,
    class_field_types: &HashMap<(String, String), String>,
) -> Option<(String, Option<LocalId>)> {
    match obj {
        Expr::LocalGet(id) => local_types.get(id).map(|cn| (cn.clone(), Some(*id))),
        Expr::This => enclosing_class.map(|cn| (cn.to_string(), None)),
        Expr::PropertyGet {
            object, property, ..
        } => {
            // Recursive resolution: get the inner receiver's class, then
            // look up the field on that class. Field-walking chains like
            // `world.commandBuffer.set(...)` benefit — without this the
            // inliner's receiver match bails at the first non-LocalGet.
            let (inner_class, _) =
                resolve_receiver_class(object, local_types, enclosing_class, class_field_types)?;
            class_field_types
                .get(&(inner_class, property.clone()))
                .cloned()
                .map(|cn| (cn, None))
        }
        _ => None,
    }
}

pub fn intersect_exact_receiver_facts(
    left: &ExactReceiverFacts,
    right: &ExactReceiverFacts,
) -> ExactReceiverFacts {
    left.iter()
        .filter_map(|(id, fact)| {
            right
                .get(id)
                .filter(|other| *other == fact)
                .map(|_| (*id, fact.clone()))
        })
        .collect()
}

pub fn apply_exact_receiver_stmt_effect(stmt: &Stmt, facts: &mut ExactReceiverFacts) {
    match stmt {
        Stmt::Let { id, init, .. } => {
            facts.remove(id);
            if let Some(init) = init {
                invalidate_exact_receivers_for_expr(init, facts);
                kill_referenced_exact_receivers(init, facts);
                if let Expr::New { class_name, .. } = init {
                    facts.insert(
                        *id,
                        ExactReceiverFact {
                            class_name: class_name.clone(),
                        },
                    );
                }
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
            invalidate_exact_receivers_for_expr(expr, facts);
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
        Stmt::PreallocateBoxes(ids) | Stmt::PreallocateTdzBoxes(ids) => {
            for id in ids {
                facts.remove(id);
            }
        }
        Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::DoWhile { .. }
        | Stmt::For { .. }
        | Stmt::Labeled { .. }
        | Stmt::Try { .. }
        | Stmt::Switch { .. } => facts.clear(),
    }
}

pub fn apply_exact_receiver_stmt_effects(stmts: &[Stmt], facts: &mut ExactReceiverFacts) {
    for stmt in stmts {
        apply_exact_receiver_stmt_effect(stmt, facts);
    }
}

pub fn clear_exact_receivers_after_global_effect(expr: &Expr, facts: &mut ExactReceiverFacts) {
    walk_expr_children(expr, &mut |child| {
        invalidate_exact_receivers_for_expr(child, facts)
    });
    facts.clear();
}

pub fn invalidate_exact_receivers_for_expr(expr: &Expr, facts: &mut ExactReceiverFacts) {
    match expr {
        Expr::Call { .. }
        | Expr::CallSpread { .. }
        | Expr::NativeMethodCall { .. }
        | Expr::StaticMethodCall { .. }
        | Expr::SuperCall(_)
        | Expr::SuperMethodCall { .. }
        | Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::ObjectAssign { .. }
        | Expr::PropertySet { .. }
        // #4126 lowers property assignments as `PutValueSet`; an
        // `obj.method = X` write at a use site must invalidate the
        // exact-receiver facts (same as PropertySet) so a subsequently
        // overridden method isn't wrongly inlined (#945 unsafe variants).
        | Expr::PutValueSet { .. }
        | Expr::PropertyUpdate { .. }
        | Expr::IndexSet { .. }
        | Expr::IndexUpdate { .. }
        | Expr::StaticFieldSet { .. }
        | Expr::ClassStaticSymbolSet { .. }
        | Expr::RegisterClassParentDynamic { .. }
        | Expr::RegisterClassStaticSymbol { .. }
        | Expr::ClassExprFresh { .. }
        | Expr::SetFunctionPrototype { .. }
        | Expr::RegisterPrototypeMethod { .. }
        | Expr::RegisterFunctionPrototypeMethod { .. }
        | Expr::ObjectDefineProperty(_, _, _)
        | Expr::ObjectDefineProperties(_, _)
        | Expr::ObjectSetPrototypeOf(_, _)
        | Expr::Delete(_)
        | Expr::ReflectSet { .. }
        | Expr::ReflectDelete { .. }
        | Expr::ReflectDefineProperty { .. } => {
            clear_exact_receivers_after_global_effect(expr, facts);
        }
        Expr::Object(_)
        | Expr::ObjectSpread { .. }
        | Expr::Array(_)
        | Expr::ArraySpread(_)
        | Expr::LocalSet(_, _)
        | Expr::GlobalSet(_, _) => {
            kill_referenced_exact_receivers(expr, facts);
        }
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures.iter()) {
                facts.remove(id);
            }
            let mut body_refs = HashSet::new();
            for stmt in body {
                collect_exact_receiver_refs_in_stmt(stmt, facts, &mut body_refs);
            }
            for id in body_refs {
                facts.remove(&id);
            }
            for param in params {
                if let Some(default) = &param.default {
                    invalidate_exact_receivers_for_expr(default, facts);
                }
            }
        }
        _ => {
            walk_expr_children(expr, &mut |child| {
                invalidate_exact_receivers_for_expr(child, facts)
            });
        }
    }
}

pub fn kill_referenced_exact_receivers(expr: &Expr, facts: &mut ExactReceiverFacts) {
    let mut refs = HashSet::new();
    collect_exact_receiver_refs_in_expr(expr, facts, &mut refs);
    for id in refs {
        facts.remove(&id);
    }
}

pub fn collect_exact_receiver_refs_in_stmt(
    stmt: &Stmt,
    facts: &ExactReceiverFacts,
    out: &mut HashSet<LocalId>,
) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(init) = init {
                collect_exact_receiver_refs_in_expr(init, facts, out);
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => {
            collect_exact_receiver_refs_in_expr(expr, facts, out);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_exact_receiver_refs_in_expr(condition, facts, out);
            for stmt in then_branch {
                collect_exact_receiver_refs_in_stmt(stmt, facts, out);
            }
            if let Some(else_branch) = else_branch {
                for stmt in else_branch {
                    collect_exact_receiver_refs_in_stmt(stmt, facts, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_exact_receiver_refs_in_expr(condition, facts, out);
            for stmt in body {
                collect_exact_receiver_refs_in_stmt(stmt, facts, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                collect_exact_receiver_refs_in_stmt(init, facts, out);
            }
            if let Some(condition) = condition {
                collect_exact_receiver_refs_in_expr(condition, facts, out);
            }
            if let Some(update) = update {
                collect_exact_receiver_refs_in_expr(update, facts, out);
            }
            for stmt in body {
                collect_exact_receiver_refs_in_stmt(stmt, facts, out);
            }
        }
        Stmt::Labeled { body, .. } => collect_exact_receiver_refs_in_stmt(body, facts, out),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for stmt in body {
                collect_exact_receiver_refs_in_stmt(stmt, facts, out);
            }
            if let Some(catch) = catch {
                for stmt in &catch.body {
                    collect_exact_receiver_refs_in_stmt(stmt, facts, out);
                }
            }
            if let Some(finally) = finally {
                for stmt in finally {
                    collect_exact_receiver_refs_in_stmt(stmt, facts, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_exact_receiver_refs_in_expr(discriminant, facts, out);
            for case in cases {
                if let Some(test) = &case.test {
                    collect_exact_receiver_refs_in_expr(test, facts, out);
                }
                for stmt in &case.body {
                    collect_exact_receiver_refs_in_stmt(stmt, facts, out);
                }
            }
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_)
        | Stmt::PreallocateTdzBoxes(_) => {}
    }
}

pub fn collect_exact_receiver_refs_in_expr(
    expr: &Expr,
    facts: &ExactReceiverFacts,
    out: &mut HashSet<LocalId>,
) {
    match expr {
        Expr::LocalGet(id) | Expr::LocalSet(id, _) if facts.contains_key(id) => {
            out.insert(*id);
        }
        Expr::Update { id, .. } if facts.contains_key(id) => {
            out.insert(*id);
        }
        Expr::Closure {
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            for id in captures.iter().chain(mutable_captures.iter()) {
                if facts.contains_key(id) {
                    out.insert(*id);
                }
            }
            for param in params {
                if let Some(default) = &param.default {
                    collect_exact_receiver_refs_in_expr(default, facts, out);
                }
            }
            for stmt in body {
                collect_exact_receiver_refs_in_stmt(stmt, facts, out);
            }
            return;
        }
        _ => {}
    }
    walk_expr_children(expr, &mut |child| {
        collect_exact_receiver_refs_in_expr(child, facts, out)
    });
}
