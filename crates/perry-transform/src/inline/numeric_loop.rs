//! Keep numeric field loops available for guarded region versioning.
//!
//! Codegen cannot lower a closure creation twice under the same function ID.
//! Moving one into a loop by inlining therefore prevents the entire loop from
//! being versioned. Leave that callee behind a normal call instead: region
//! planning already treats the call as an effect boundary and rechecks its
//! receiver facts before subsequent field operations.

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
use perry_hir::types::FuncId;
use perry_hir::walker::{stmt_any_expr, walk_expr_children};
use perry_hir::{Expr, Function, Stmt};

fn expr_any(expr: &Expr, predicate: fn(&Expr) -> bool) -> bool {
    if predicate(expr) {
        return true;
    }
    let mut found = false;
    walk_expr_children(expr, &mut |child| found |= expr_any(child, predicate));
    found
}

pub(super) fn contains_field_arithmetic(stmts: &[Stmt]) -> bool {
    // Accumulate both facts in one traversal. Re-scanning each binary's
    // children would make a long local-only arithmetic chain quadratic.
    fn inspect(expr: &Expr) -> (bool, bool) {
        let mut has_field = matches!(expr, Expr::PropertyGet { object, .. }
            if matches!(object.as_ref(), Expr::LocalGet(_) | Expr::This));
        let mut has_arithmetic = false;
        walk_expr_children(expr, &mut |child| {
            let (field, arithmetic) = inspect(child);
            has_field |= field;
            has_arithmetic |= arithmetic;
        });
        has_arithmetic |= has_field && matches!(expr, Expr::Binary { .. });
        (has_field, has_arithmetic)
    }
    stmts
        .iter()
        .any(|stmt| stmt_any_expr(stmt, &mut |expr| inspect(expr).1))
}

pub(super) fn introduces_closure(function: &Function) -> bool {
    fn closure(expr: &Expr) -> bool {
        matches!(expr, Expr::Closure { .. })
    }
    function
        .body
        .iter()
        .any(|stmt| stmt_any_expr(stmt, &mut |expr| expr_any(expr, closure)))
        || function
            .params
            .iter()
            .filter_map(|param| param.default.as_ref())
            .any(|expr| expr_any(expr, closure))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inline::call_inliner::inline_calls_in_stmts;
    use crate::inline::ExactReceiverFacts;
    use perry_hir::types::Type;
    use perry_hir::BinaryOp;

    fn function(id: FuncId, body: Vec<Stmt>) -> Function {
        Function {
            id,
            name: format!("f{id}"),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Type::Void,
            body,
            is_async: false,
            is_generator: false,
            is_exported: false,
            captures: Vec::new(),
            decorators: Vec::new(),
            was_plain_async: false,
            was_unrolled: false,
            is_strict: true,
        }
    }

    fn closure() -> Expr {
        Expr::Closure {
            func_id: 99,
            params: Vec::new(),
            return_type: Type::Number,
            body: vec![Stmt::Return(Some(Expr::Integer(3)))],
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: true,
        }
    }

    fn call(id: FuncId) -> Stmt {
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::FuncRef(id)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })
    }

    fn arithmetic(field: bool) -> Stmt {
        Stmt::Expr(Expr::Binary {
            op: BinaryOp::Mul,
            left: Box::new(if field {
                Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(1)),
                    property: "a".into(),
                    byte_offset: 0,
                }
            } else {
                Expr::LocalGet(1)
            }),
            right: Box::new(Expr::Integer(1)),
        })
    }

    fn run(stmts: &mut Vec<Stmt>) {
        let functions = HashMap::from([
            // Closure nested in a dedicated HIR expression and an if branch.
            (
                1,
                function(
                    1,
                    vec![Stmt::If {
                        condition: Expr::Bool(true),
                        then_branch: vec![Stmt::Expr(Expr::ArrayMap {
                            array: Box::new(Expr::LocalGet(7)),
                            callback: Box::new(closure()),
                        })],
                        else_branch: None,
                    }],
                ),
            ),
            (
                2,
                function(
                    2,
                    vec![Stmt::Expr(Expr::LocalSet(8, Box::new(Expr::Integer(123))))],
                ),
            ),
            (3, function(3, vec![call(1)])),
        ]);
        inline_calls_in_stmts(
            stmts,
            &functions,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
            &mut ExactReceiverFacts::new(),
            &mut 100,
            None,
            &HashMap::new(),
            false,
        );
    }

    fn has_call(stmts: &[Stmt], id: FuncId) -> bool {
        stmts.iter().any(|stmt| stmt_any_expr(stmt, &mut |expr| {
            fn check(expr: &Expr, id: FuncId) -> bool {
                if matches!(expr, Expr::Call { callee, .. } if matches!(callee.as_ref(), Expr::FuncRef(n) if *n == id)) {
                    return true;
                }
                let mut found = false;
                walk_expr_children(expr, &mut |child| found |= check(child, id));
                found
            }
            check(expr, id)
        }))
    }

    #[test]
    fn field_loop_retains_closure_call_but_inlines_tiny_calls() {
        for kind in 0..3 {
            let body = vec![arithmetic(true), call(1), call(2)];
            let stmt = match kind {
                0 => Stmt::For {
                    init: None,
                    condition: Some(Expr::Bool(true)),
                    update: None,
                    body,
                },
                1 => Stmt::While {
                    condition: Expr::Bool(true),
                    body,
                },
                _ => Stmt::DoWhile {
                    condition: Expr::Bool(true),
                    body,
                },
            };
            let mut stmts = vec![stmt];
            run(&mut stmts);
            assert!(has_call(&stmts, 1));
            assert!(!has_call(&stmts, 2));
            assert!(!introduces_closure(&function(5, stmts)));
        }
    }

    #[test]
    fn field_loop_preserves_helper_boundary_after_recursive_inline() {
        let mut stmts = vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![arithmetic(true), call(3)],
        }];
        run(&mut stmts);
        assert!(!has_call(&stmts, 3));
        assert!(has_call(&stmts, 1));
        assert!(!introduces_closure(&function(5, stmts)));
    }

    #[test]
    fn outside_loop_closure_call_still_inlines() {
        let mut stmts = vec![arithmetic(true), call(1)];
        run(&mut stmts);
        assert!(!has_call(&stmts, 1));
        assert!(introduces_closure(&function(5, stmts)));
    }

    #[test]
    fn loop_without_field_arithmetic_still_inlines_closure_call() {
        let mut stmts = vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![arithmetic(false), call(1)],
        }];
        run(&mut stmts);
        assert!(!has_call(&stmts, 1));
        assert!(introduces_closure(&function(5, stmts)));
    }
}
