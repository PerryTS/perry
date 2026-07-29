//! Closure collection helpers extracted from emit/mod.rs (#1102 mechanical split).
//!
//! Pure move: `collect_closures_from_stmts` / `collect_closures_from_expr`.

use perry_hir::ir::*;
use perry_hir::types::{FuncId, LocalId};

/// Recursively collect all Expr::Closure nodes from statements
pub(super) fn collect_closures_from_stmts(
    stmts: &[Stmt],
    out: &mut Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init {
                    collect_closures_from_expr(e, out);
                }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => collect_closures_from_expr(e, out),
            Stmt::Return(e) => {
                if let Some(e) = e {
                    collect_closures_from_expr(e, out);
                }
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(then_branch, out);
                if let Some(eb) = else_branch {
                    collect_closures_from_stmts(eb, out);
                }
            }
            Stmt::While { condition, body } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(body, out);
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    collect_closures_from_stmts(std::slice::from_ref(i.as_ref()), out);
                }
                if let Some(c) = condition {
                    collect_closures_from_expr(c, out);
                }
                if let Some(u) = update {
                    collect_closures_from_expr(u, out);
                }
                collect_closures_from_stmts(body, out);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                collect_closures_from_stmts(body, out);
                if let Some(c) = catch {
                    collect_closures_from_stmts(&c.body, out);
                }
                if let Some(f) = finally {
                    collect_closures_from_stmts(f, out);
                }
            }
            Stmt::Switch {
                discriminant,
                cases,
            } => {
                collect_closures_from_expr(discriminant, out);
                for case in cases {
                    if let Some(t) = &case.test {
                        collect_closures_from_expr(t, out);
                    }
                    collect_closures_from_stmts(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect Expr::Closure from an expression tree
pub(super) fn collect_closures_from_expr(
    expr: &Expr,
    out: &mut Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)>,
) {
    match expr {
        Expr::Closure {
            func_id,
            params,
            body,
            captures,
            mutable_captures,
            ..
        } => {
            out.push((
                *func_id,
                params.clone(),
                body.clone(),
                captures.clone(),
                mutable_captures.clone(),
            ));
            // Also collect nested closures
            collect_closures_from_stmts(body, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_closures_from_expr(callee, out);
            for a in args {
                collect_closures_from_expr(a, out);
            }
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            collect_closures_from_expr(left, out);
            collect_closures_from_expr(right, out);
        }
        Expr::Unary { operand, .. }
        | Expr::TypeOf(operand)
        | Expr::Void(operand)
        | Expr::Await(operand) => {
            collect_closures_from_expr(operand, out);
        }
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            collect_closures_from_expr(val, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            collect_closures_from_expr(condition, out);
            collect_closures_from_expr(then_expr, out);
            collect_closures_from_expr(else_expr, out);
        }
        Expr::Object(fields) => {
            for (_, v) in fields {
                collect_closures_from_expr(v, out);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_closures_from_expr(e, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            collect_closures_from_expr(object, out);
        }
        Expr::PropertySet { object, value, .. } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(index, out);
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(index, out);
            collect_closures_from_expr(value, out);
        }
        // #5016: PutValueSet is the lowered form of `obj.prop = v` / `obj[i] = v`.
        // Recurse into all operands so a closure used as a value/key/target
        // (e.g. `obj.handler = () => {}`) is collected and emitted.
        Expr::PutValueSet {
            target,
            key,
            value,
            receiver,
            ..
        } => {
            collect_closures_from_expr(target, out);
            collect_closures_from_expr(key, out);
            collect_closures_from_expr(value, out);
            collect_closures_from_expr(receiver, out);
        }
        Expr::NativeMethodCall { args, object, .. } => {
            if let Some(o) = object {
                collect_closures_from_expr(o, out);
            }
            for a in args {
                collect_closures_from_expr(a, out);
            }
        }
        Expr::New { args, .. } => {
            for a in args {
                collect_closures_from_expr(a, out);
            }
        }
        Expr::ArrayMap { array, callback }
        | Expr::ArrayFilter { array, callback }
        | Expr::ArrayForEach { array, callback }
        | Expr::ArrayFind { array, callback }
        | Expr::ArrayFindIndex { array, callback }
        | Expr::ArraySort {
            array,
            comparator: callback,
        } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(callback, out);
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
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(callback, out);
            if let Some(i) = initial {
                collect_closures_from_expr(i, out);
            }
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closures_from_expr(array, out);
            if let Some(c) = comparator {
                collect_closures_from_expr(c, out);
            }
        }
        Expr::ArrayToSpliced {
            array,
            start,
            delete_count,
            items,
        } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(start, out);
            collect_closures_from_expr(delete_count, out);
            for item in items {
                collect_closures_from_expr(item, out);
            }
        }
        Expr::ArrayWith {
            array,
            index,
            value,
        } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(index, out);
            collect_closures_from_expr(value, out);
        }
        Expr::ArrayCopyWithin {
            target, start, end, ..
        } => {
            collect_closures_from_expr(target, out);
            collect_closures_from_expr(start, out);
            if let Some(e) = end {
                collect_closures_from_expr(e, out);
            }
        }
        Expr::ArrayToReversed { array } => {
            collect_closures_from_expr(array, out);
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closures_from_expr(array, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs {
                collect_closures_from_expr(e, out);
            }
        }
        _ => {}
    }
}
