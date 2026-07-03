//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

/// Post-lowering pass that widens every `Expr::Closure`'s `mutable_captures`
/// to include any capture that is assigned to inside a sibling closure in the
/// same lexical scope. Then recurses into each closure body so nested scopes
/// get the same treatment. This ensures that when multiple closures share a
/// captured binding and any one of them mutates it, all of them treat it as
/// boxed so reads and writes observe the same storage slot.
pub(crate) fn widen_mutable_captures_stmts(stmts: &mut [Stmt]) {
    // Tier 4.3 (v0.5.336): three independent read passes fused into a
    // single iteration over `stmts`. Pre-fix this was three separate
    // `for stmt in stmts.iter()` loops back-to-back, each populating
    // its own HashSet. The collectors don't depend on each other's
    // outputs (they read disjoint Expr/Stmt fields), so calling all
    // three per stmt is equivalent and saves 2 full slice traversals
    // per scope. The mutating pass below still runs separately because
    // it depends on the union of all three sets.
    //
    // Also detects variables that are captured by closures AND assigned
    // at the scope level (not inside a closure). This handles the pattern:
    //   let x = 0;
    //   fns.push(() => x);
    //   x = 10;               // assignment at scope level
    //   fns.push(() => x);
    // All closures should see the final value of x (capture-by-reference).
    let mut scope_mutable: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    let mut scope_captured: std::collections::HashSet<LocalId> = std::collections::HashSet::new();
    let mut scope_assigned_at_level: std::collections::HashSet<LocalId> =
        std::collections::HashSet::new();
    for stmt in stmts.iter() {
        collect_closure_assigned_stmt(stmt, &mut scope_mutable);
        collect_closure_captures_stmt(stmt, &mut scope_captured);
        collect_scope_level_assigns_stmt(stmt, &mut scope_assigned_at_level);
    }
    for id in &scope_captured {
        if scope_assigned_at_level.contains(id) {
            scope_mutable.insert(*id);
        }
    }
    for stmt in stmts.iter_mut() {
        widen_mutable_captures_stmt(stmt, &scope_mutable);
    }
}

/// Insert preallocated boxes when a closure is created before a later lexical
/// binding that it captures. Later lowering may split/in-line expressions into
/// `let getter = () => value; let value = ...`; the closure must capture the
/// future binding's box, not its current undefined snapshot.
pub(crate) fn preallocate_forward_captured_lets_stmts(stmts: &mut Vec<Stmt>) {
    let mut forward = collect_forward_captured_let_ids(stmts);
    if !forward.is_empty() {
        forward.sort();
        forward.dedup();
        match stmts.first_mut() {
            Some(Stmt::PreallocateBoxes(existing)) => {
                for id in forward {
                    if !existing.contains(&id) {
                        existing.push(id);
                    }
                }
                existing.sort();
            }
            _ => stmts.insert(0, Stmt::PreallocateBoxes(forward)),
        }
    }

    for stmt in stmts.iter_mut() {
        preallocate_forward_captured_lets_stmt(stmt);
    }
}

fn collect_forward_captured_let_ids(stmts: &[Stmt]) -> Vec<LocalId> {
    let mut scope_lets = std::collections::HashSet::new();
    for stmt in stmts {
        if let Stmt::Let { id, .. } = stmt {
            scope_lets.insert(*id);
        }
    }

    let mut declared = std::collections::HashSet::new();
    let mut out = std::collections::HashSet::new();
    for stmt in stmts {
        let mut captures = std::collections::HashSet::new();
        collect_closure_captures_stmt(stmt, &mut captures);
        for id in captures {
            if scope_lets.contains(&id) && !declared.contains(&id) {
                out.insert(id);
            }
        }
        if let Stmt::Let { id, .. } = stmt {
            declared.insert(*id);
        }
    }

    out.into_iter().collect()
}

fn preallocate_forward_captured_lets_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            preallocate_forward_captured_lets_stmts(then_branch);
            if let Some(else_branch) = else_branch {
                preallocate_forward_captured_lets_stmts(else_branch);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            preallocate_forward_captured_lets_stmts(body);
        }
        Stmt::For { init, body, .. } => {
            if let Some(init) = init {
                preallocate_forward_captured_lets_stmt(init);
            }
            preallocate_forward_captured_lets_stmts(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            preallocate_forward_captured_lets_stmts(body);
            if let Some(catch) = catch {
                preallocate_forward_captured_lets_stmts(&mut catch.body);
            }
            if let Some(finally) = finally {
                preallocate_forward_captured_lets_stmts(finally);
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                preallocate_forward_captured_lets_stmts(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            preallocate_forward_captured_lets_stmt(body);
        }
        _ => {}
    }
}

fn widen_mutable_captures_stmt(
    stmt: &mut Stmt,
    scope_mutable: &std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Expr(expr) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Return(Some(expr)) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::Throw(expr) => widen_mutable_captures_expr(expr, scope_mutable),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            widen_mutable_captures_expr(condition, scope_mutable);
            widen_mutable_captures_stmts(then_branch);
            if let Some(else_stmts) = else_branch {
                widen_mutable_captures_stmts(else_stmts);
            }
        }
        Stmt::While { condition, body } => {
            widen_mutable_captures_expr(condition, scope_mutable);
            widen_mutable_captures_stmts(body);
        }
        Stmt::DoWhile { body, condition } => {
            widen_mutable_captures_stmts(body);
            widen_mutable_captures_expr(condition, scope_mutable);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                widen_mutable_captures_stmt(init_stmt, scope_mutable);
            }
            if let Some(cond) = condition {
                widen_mutable_captures_expr(cond, scope_mutable);
            }
            if let Some(upd) = update {
                widen_mutable_captures_expr(upd, scope_mutable);
            }
            widen_mutable_captures_stmts(body);
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            widen_mutable_captures_stmts(body);
            if let Some(catch_clause) = catch {
                widen_mutable_captures_stmts(&mut catch_clause.body);
            }
            if let Some(finally_stmts) = finally {
                widen_mutable_captures_stmts(finally_stmts);
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            widen_mutable_captures_expr(discriminant, scope_mutable);
            for case in cases {
                if let Some(test) = &mut case.test {
                    widen_mutable_captures_expr(test, scope_mutable);
                }
                widen_mutable_captures_stmts(&mut case.body);
            }
        }
        Stmt::Labeled { body, .. } => {
            widen_mutable_captures_stmt(body, scope_mutable);
        }
        _ => {}
    }
}

fn widen_mutable_captures_expr(
    expr: &mut Expr,
    scope_mutable: &std::collections::HashSet<LocalId>,
) {
    match expr {
        Expr::Closure {
            captures,
            mutable_captures,
            body,
            ..
        } => {
            let mut mset: std::collections::HashSet<LocalId> =
                mutable_captures.iter().copied().collect();
            for id in captures.iter() {
                if scope_mutable.contains(id) {
                    mset.insert(*id);
                }
            }
            let mut new_mutable: Vec<LocalId> = mset.into_iter().collect();
            new_mutable.sort();
            *mutable_captures = new_mutable;

            // Recurse into the closure body so nested closures get a fresh
            // scope-relative widening.
            widen_mutable_captures_stmts(body);
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            widen_mutable_captures_expr(left, scope_mutable);
            widen_mutable_captures_expr(right, scope_mutable);
        }
        Expr::Unary { operand, .. } => widen_mutable_captures_expr(operand, scope_mutable),
        Expr::Call { callee, args, .. } => {
            widen_mutable_captures_expr(callee, scope_mutable);
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            widen_mutable_captures_expr(callee, scope_mutable);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        widen_mutable_captures_expr(e, scope_mutable)
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        widen_mutable_captures_expr(x, scope_mutable)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                widen_mutable_captures_expr(v, scope_mutable);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                widen_mutable_captures_expr(v, scope_mutable);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            widen_mutable_captures_expr(condition, scope_mutable);
            widen_mutable_captures_expr(then_expr, scope_mutable);
            widen_mutable_captures_expr(else_expr, scope_mutable);
        }
        Expr::PropertyGet { object, .. } => widen_mutable_captures_expr(object, scope_mutable),
        Expr::PropertySet { object, value, .. } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::PropertyUpdate { object, .. } => widen_mutable_captures_expr(object, scope_mutable),
        Expr::IndexGet { object, index } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::IndexUpdate { object, index, .. } => {
            widen_mutable_captures_expr(object, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
        }
        Expr::New { args, .. } => {
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            widen_mutable_captures_expr(callee, scope_mutable);
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => {
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) | Expr::Delete(inner) => {
            widen_mutable_captures_expr(inner, scope_mutable);
        }
        Expr::InstanceOf { expr, .. } => widen_mutable_captures_expr(expr, scope_mutable),
        Expr::In { property, object } => {
            widen_mutable_captures_expr(property, scope_mutable);
            widen_mutable_captures_expr(object, scope_mutable);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(callback, scope_mutable);
        }
        Expr::ArraySort { array, comparator } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(comparator, scope_mutable);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(callback, scope_mutable);
            if let Some(init) = initial {
                widen_mutable_captures_expr(init, scope_mutable);
            }
        }
        Expr::ArrayToReversed { array } | Expr::ArrayReverseValue { receiver: array } => {
            widen_mutable_captures_expr(array, scope_mutable);
        }
        Expr::ArrayToSorted { array, comparator } => {
            widen_mutable_captures_expr(array, scope_mutable);
            if let Some(cmp) = comparator {
                widen_mutable_captures_expr(cmp, scope_mutable);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(start, scope_mutable);
            widen_mutable_captures_expr(delete_count, scope_mutable);
            for item in items {
                widen_mutable_captures_expr(item, scope_mutable);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            widen_mutable_captures_expr(array, scope_mutable);
            widen_mutable_captures_expr(index, scope_mutable);
            widen_mutable_captures_expr(value, scope_mutable);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            widen_mutable_captures_expr(target, scope_mutable);
            widen_mutable_captures_expr(start, scope_mutable);
            if let Some(e) = end {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            widen_mutable_captures_expr(receiver, scope_mutable);
            widen_mutable_captures_expr(target, scope_mutable);
            widen_mutable_captures_expr(start, scope_mutable);
            if let Some(e) = end {
                widen_mutable_captures_expr(e, scope_mutable);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            widen_mutable_captures_expr(array, scope_mutable);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                widen_mutable_captures_expr(obj, scope_mutable);
            }
            for arg in args {
                widen_mutable_captures_expr(arg, scope_mutable);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            widen_mutable_captures_expr(closure, scope_mutable)
        }
        Expr::ArrayPush { value, .. } | Expr::ArrayPushSpread { source: value, .. } => {
            widen_mutable_captures_expr(value, scope_mutable);
        }
        _ => {}
    }
}

/// Walk a statement collecting the set of LocalIds that are assigned to
/// inside any `Expr::Closure` reachable from it (including nested closures).
/// This is the "mutably shared" set at the enclosing lexical scope.
fn collect_closure_assigned_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_closure_assigned_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_assigned_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_assigned_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_assigned_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_assigned_expr(condition, out);
            for s in then_branch {
                collect_closure_assigned_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_closure_assigned_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_assigned_expr(condition, out);
            for s in body {
                collect_closure_assigned_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_closure_assigned_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_closure_assigned_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_closure_assigned_expr(upd, out);
            }
            for s in body {
                collect_closure_assigned_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_assigned_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    collect_closure_assigned_stmt(s, out);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_closure_assigned_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_assigned_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_closure_assigned_expr(test, out);
                }
                for s in &case.body {
                    collect_closure_assigned_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_assigned_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_assigned_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::Closure { body, .. } => {
            // Any LocalSet/Update inside this closure body (or nested closures
            // within it) counts as "assigned in a closure at our scope".
            for stmt in body {
                collect_closure_assigned_in_closure_body_stmt(stmt, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_assigned_expr(left, out);
            collect_closure_assigned_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_assigned_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_assigned_expr(callee, out);
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_closure_assigned_expr(callee, out);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => collect_closure_assigned_expr(e, out),
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        collect_closure_assigned_expr(x, out)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closure_assigned_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_closure_assigned_expr(v, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closure_assigned_expr(condition, out);
            collect_closure_assigned_expr(then_expr, out);
            collect_closure_assigned_expr(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => collect_closure_assigned_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::PropertyUpdate { object, .. } => collect_closure_assigned_expr(object, out),
        Expr::IndexGet { object, index } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(index, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_closure_assigned_expr(object, out);
            collect_closure_assigned_expr(index, out);
        }
        Expr::New { args, .. } => {
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            collect_closure_assigned_expr(callee, out);
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => {
            collect_closure_assigned_expr(value, out);
        }
        Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) | Expr::Delete(inner) => {
            collect_closure_assigned_expr(inner, out);
        }
        Expr::InstanceOf { expr, .. } => collect_closure_assigned_expr(expr, out),
        Expr::In { property, object } => {
            collect_closure_assigned_expr(property, out);
            collect_closure_assigned_expr(object, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(callback, out);
        }
        Expr::ArraySort { array, comparator } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(comparator, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(callback, out);
            if let Some(init) = initial {
                collect_closure_assigned_expr(init, out);
            }
        }
        Expr::ArrayToReversed { array } | Expr::ArrayReverseValue { receiver: array } => {
            collect_closure_assigned_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_assigned_expr(array, out);
            if let Some(cmp) = comparator {
                collect_closure_assigned_expr(cmp, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(start, out);
            collect_closure_assigned_expr(delete_count, out);
            for item in items {
                collect_closure_assigned_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closure_assigned_expr(array, out);
            collect_closure_assigned_expr(index, out);
            collect_closure_assigned_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closure_assigned_expr(target, out);
            collect_closure_assigned_expr(start, out);
            if let Some(e) = end {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            collect_closure_assigned_expr(receiver, out);
            collect_closure_assigned_expr(target, out);
            collect_closure_assigned_expr(start, out);
            if let Some(e) = end {
                collect_closure_assigned_expr(e, out);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_assigned_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_closure_assigned_expr(obj, out);
            }
            for arg in args {
                collect_closure_assigned_expr(arg, out);
            }
        }
        Expr::JsCreateCallback { closure, .. } => collect_closure_assigned_expr(closure, out),
        _ => {}
    }
}

/// Collect all LocalIds that appear in the `captures` list of any closure in the scope.
fn collect_closure_captures_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_closure_captures_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_captures_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_captures_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_captures_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_captures_expr(condition, out);
            for s in then_branch {
                collect_closure_captures_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_closure_captures_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_captures_expr(condition, out);
            for s in body {
                collect_closure_captures_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_closure_captures_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_closure_captures_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_closure_captures_expr(upd, out);
            }
            for s in body {
                collect_closure_captures_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_captures_stmt(s, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    collect_closure_captures_stmt(s, out);
                }
            }
            if let Some(fs) = finally {
                for s in fs {
                    collect_closure_captures_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_captures_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_closure_captures_expr(test, out);
                }
                for s in &case.body {
                    collect_closure_captures_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_captures_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_captures_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::Closure { captures, body, .. } => {
            for id in captures {
                out.insert(*id);
            }
            // Also recurse into nested closures
            for stmt in body {
                collect_closure_captures_stmt(stmt, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_captures_expr(left, out);
            collect_closure_captures_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_captures_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_captures_expr(callee, out);
            for arg in args {
                collect_closure_captures_expr(arg, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_closure_captures_expr(callee, out);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => collect_closure_captures_expr(e, out),
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        collect_closure_captures_expr(x, out)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closure_captures_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_closure_captures_expr(v, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closure_captures_expr(condition, out);
            collect_closure_captures_expr(then_expr, out);
            collect_closure_captures_expr(else_expr, out);
        }
        Expr::LocalSet(_, value) | Expr::GlobalSet(_, value) => {
            collect_closure_captures_expr(value, out);
        }
        Expr::PropertyGet { object, .. } => collect_closure_captures_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closure_captures_expr(object, out);
            collect_closure_captures_expr(index, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::New { args, .. } | Expr::NewDynamic { args, .. } => {
            for arg in args {
                collect_closure_captures_expr(arg, out);
            }
        }
        Expr::ArrayPush { value, .. }
        | Expr::Await(value)
        | Expr::TypeOf(value)
        | Expr::Void(value)
        | Expr::Delete(value) => {
            collect_closure_captures_expr(value, out);
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(callback, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(callback, out);
            if let Some(init) = initial {
                collect_closure_captures_expr(init, out);
            }
        }
        Expr::ArrayToReversed { array } | Expr::ArrayReverseValue { receiver: array } => {
            collect_closure_captures_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_captures_expr(array, out);
            if let Some(cmp) = comparator {
                collect_closure_captures_expr(cmp, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(start, out);
            collect_closure_captures_expr(delete_count, out);
            for item in items {
                collect_closure_captures_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closure_captures_expr(array, out);
            collect_closure_captures_expr(index, out);
            collect_closure_captures_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closure_captures_expr(target, out);
            collect_closure_captures_expr(start, out);
            if let Some(e) = end {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            collect_closure_captures_expr(receiver, out);
            collect_closure_captures_expr(target, out);
            collect_closure_captures_expr(start, out);
            if let Some(e) = end {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_captures_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_closure_captures_expr(obj, out);
            }
            for arg in args {
                collect_closure_captures_expr(arg, out);
            }
        }
        Expr::JsCreateCallback { closure, .. } => collect_closure_captures_expr(closure, out),
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closure_captures_expr(e, out);
            }
        }
        // A fresh class expression can hold closures in its static
        // initializers and in the captured-args snapshot. This path is
        // reachable via `class X extends Y {}` bindings, so a closure
        // capturing a forward-declared `let` here must be seen or its box
        // won't be preallocated.
        Expr::ClassExprFresh {
            named_statics,
            symbol_statics,
            captured_args,
            ..
        } => {
            for (_, e) in named_statics {
                collect_closure_captures_expr(e, out);
            }
            for (k, v) in symbol_statics {
                collect_closure_captures_expr(k, out);
                collect_closure_captures_expr(v, out);
            }
            for e in captured_args {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::InstanceOf { expr, ty_expr, .. } => {
            collect_closure_captures_expr(expr, out);
            if let Some(te) = ty_expr {
                collect_closure_captures_expr(te, out);
            }
        }
        Expr::In { property, object } => {
            collect_closure_captures_expr(property, out);
            collect_closure_captures_expr(object, out);
        }
        Expr::TaggedTemplateStrings { cooked, .. } => {
            for e in cooked {
                collect_closure_captures_expr(e, out);
            }
        }
        Expr::TemplateRaw(inner) => collect_closure_captures_expr(inner, out),
        _ => {}
    }
}

/// Collect LocalIds that are assigned to at the current scope level
/// (via LocalSet or Update), but NOT inside closure bodies.
fn collect_scope_level_assigns_stmt(stmt: &Stmt, out: &mut std::collections::HashSet<LocalId>) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_scope_level_assigns_expr(expr, out),
        Stmt::Expr(expr) => collect_scope_level_assigns_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_scope_level_assigns_expr(expr, out),
        Stmt::Throw(expr) => collect_scope_level_assigns_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_scope_level_assigns_expr(condition, out);
            for s in then_branch {
                collect_scope_level_assigns_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_scope_level_assigns_expr(condition, out);
            for s in body {
                collect_scope_level_assigns_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_scope_level_assigns_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_scope_level_assigns_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_scope_level_assigns_expr(upd, out);
            }
            for s in body {
                collect_scope_level_assigns_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_scope_level_assigns_stmt(s, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
            if let Some(fs) = finally {
                for s in fs {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_scope_level_assigns_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_scope_level_assigns_expr(test, out);
                }
                for s in &case.body {
                    collect_scope_level_assigns_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_scope_level_assigns_stmt(body, out),
        _ => {}
    }
}

fn collect_scope_level_assigns_expr(expr: &Expr, out: &mut std::collections::HashSet<LocalId>) {
    match expr {
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            collect_scope_level_assigns_expr(value, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        // Do NOT recurse into closures — we only want scope-level assignments
        Expr::Closure { .. } => {}
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_scope_level_assigns_expr(left, out);
            collect_scope_level_assigns_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_scope_level_assigns_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_scope_level_assigns_expr(callee, out);
            for arg in args {
                collect_scope_level_assigns_expr(arg, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_scope_level_assigns_expr(condition, out);
            collect_scope_level_assigns_expr(then_expr, out);
            collect_scope_level_assigns_expr(else_expr, out);
        }
        _ => {}
    }
}

/// Walk a closure body collecting every LocalSet/Update target AND any
/// assigns inside nested closures within this body.
fn collect_closure_assigned_in_closure_body_stmt(
    stmt: &Stmt,
    out: &mut std::collections::HashSet<LocalId>,
) {
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        } => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Expr(expr) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Return(Some(expr)) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::Throw(expr) => collect_closure_assigned_in_body_expr(expr, out),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_closure_assigned_in_body_expr(condition, out);
            for s in then_branch {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
            if let Some(else_stmts) = else_branch {
                for s in else_stmts {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            collect_closure_assigned_in_body_expr(condition, out);
            for s in body {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init {
                collect_closure_assigned_in_closure_body_stmt(init_stmt, out);
            }
            if let Some(cond) = condition {
                collect_closure_assigned_in_body_expr(cond, out);
            }
            if let Some(upd) = update {
                collect_closure_assigned_in_body_expr(upd, out);
            }
            for s in body {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_assigned_in_closure_body_stmt(s, out);
            }
            if let Some(catch_clause) = catch {
                for s in &catch_clause.body {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
            if let Some(finally_stmts) = finally {
                for s in finally_stmts {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            collect_closure_assigned_in_body_expr(discriminant, out);
            for case in cases {
                if let Some(ref test) = case.test {
                    collect_closure_assigned_in_body_expr(test, out);
                }
                for s in &case.body {
                    collect_closure_assigned_in_closure_body_stmt(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_assigned_in_closure_body_stmt(body, out),
        _ => {}
    }
}

fn collect_closure_assigned_in_body_expr(
    expr: &Expr,
    out: &mut std::collections::HashSet<LocalId>,
) {
    match expr {
        Expr::LocalSet(id, value) => {
            out.insert(*id);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::Update { id, .. } => {
            out.insert(*id);
        }
        Expr::Closure { body, .. } => {
            for stmt in body {
                collect_closure_assigned_in_closure_body_stmt(stmt, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closure_assigned_in_body_expr(left, out);
            collect_closure_assigned_in_body_expr(right, out);
        }
        Expr::Unary { operand, .. } => collect_closure_assigned_in_body_expr(operand, out),
        Expr::Call { callee, args, .. } => {
            collect_closure_assigned_in_body_expr(callee, out);
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::CallSpread { callee, args, .. } => {
            collect_closure_assigned_in_body_expr(callee, out);
            for arg in args {
                match arg {
                    CallArg::Expr(e) | CallArg::Spread(e) => {
                        collect_closure_assigned_in_body_expr(e, out)
                    }
                }
            }
        }
        Expr::Array(elements) => {
            for e in elements {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArraySpread(elements) => {
            for e in elements {
                match e {
                    ArrayElement::Expr(x) | ArrayElement::Spread(x) => {
                        collect_closure_assigned_in_body_expr(x, out)
                    }
                    ArrayElement::Hole => {}
                }
            }
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closure_assigned_in_body_expr(v, out);
            }
        }
        Expr::ObjectSpread { parts } => {
            for (_, v) in parts {
                collect_closure_assigned_in_body_expr(v, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closure_assigned_in_body_expr(condition, out);
            collect_closure_assigned_in_body_expr(then_expr, out);
            collect_closure_assigned_in_body_expr(else_expr, out);
        }
        Expr::PropertyGet { object, .. } => collect_closure_assigned_in_body_expr(object, out),
        Expr::PropertySet { object, value, .. } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::PropertyUpdate { object, .. } => collect_closure_assigned_in_body_expr(object, out),
        Expr::IndexGet { object, index } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(index, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::IndexUpdate { object, index, .. } => {
            collect_closure_assigned_in_body_expr(object, out);
            collect_closure_assigned_in_body_expr(index, out);
        }
        Expr::New { args, .. } => {
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            collect_closure_assigned_in_body_expr(callee, out);
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::GlobalSet(_, value) => collect_closure_assigned_in_body_expr(value, out),
        Expr::Await(inner) | Expr::TypeOf(inner) | Expr::Void(inner) | Expr::Delete(inner) => {
            collect_closure_assigned_in_body_expr(inner, out);
        }
        Expr::InstanceOf { expr, .. } => collect_closure_assigned_in_body_expr(expr, out),
        Expr::In { property, object } => {
            collect_closure_assigned_in_body_expr(property, out);
            collect_closure_assigned_in_body_expr(object, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArrayForEach { array, callback }
        | Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySome { array, callback }
        | Expr::ArrayEvery { array, callback }
        | Expr::ArrayFlatMap { array, callback } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(callback, out);
        }
        Expr::ArraySort { array, comparator } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(comparator, out);
        }
        Expr::ArrayReduce {
            array,
            callback,
            initial,
        }
        | Expr::ArrayReduceRight {
            array,
            callback,
            initial,
        } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(callback, out);
            if let Some(init) = initial {
                collect_closure_assigned_in_body_expr(init, out);
            }
        }
        Expr::ArrayToReversed { array } | Expr::ArrayReverseValue { receiver: array } => {
            collect_closure_assigned_in_body_expr(array, out);
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closure_assigned_in_body_expr(array, out);
            if let Some(cmp) = comparator {
                collect_closure_assigned_in_body_expr(cmp, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(start, out);
            collect_closure_assigned_in_body_expr(delete_count, out);
            for item in items {
                collect_closure_assigned_in_body_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closure_assigned_in_body_expr(array, out);
            collect_closure_assigned_in_body_expr(index, out);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closure_assigned_in_body_expr(target, out);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(e) = end {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArrayCopyWithinValue {
            receiver,
            target,
            start,
            end,
        } => {
            collect_closure_assigned_in_body_expr(receiver, out);
            collect_closure_assigned_in_body_expr(target, out);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(e) = end {
                collect_closure_assigned_in_body_expr(e, out);
            }
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closure_assigned_in_body_expr(array, out);
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(obj) = object {
                collect_closure_assigned_in_body_expr(obj, out);
            }
            for arg in args {
                collect_closure_assigned_in_body_expr(arg, out);
            }
        }
        Expr::JsCreateCallback { closure, .. } => {
            collect_closure_assigned_in_body_expr(closure, out)
        }
        // Array mutation methods may reallocate the array pointer, so they
        // count as assignments to the array_id for mutable-capture widening.
        Expr::ArrayPush { array_id, value }
        | Expr::ArrayUnshift { array_id, value }
        | Expr::ArrayPushSpread {
            array_id,
            source: value,
        } => {
            out.insert(*array_id);
            collect_closure_assigned_in_body_expr(value, out);
        }
        Expr::ArrayPop(array_id) | Expr::ArrayShift(array_id) => {
            out.insert(*array_id);
        }
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            out.insert(*array_id);
            collect_closure_assigned_in_body_expr(start, out);
            if let Some(dc) = delete_count {
                collect_closure_assigned_in_body_expr(dc, out);
            }
            for item in items {
                collect_closure_assigned_in_body_expr(item, out);
            }
        }
        _ => {}
    }
}
