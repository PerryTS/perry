//! Outline the compiler-owned completion protocol while retaining its HIR.
use perry_hir::{CompareOp, Expr, Stmt};

fn pure(expr: &Expr) -> bool {
    match expr {
        Expr::LocalGet(_)
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Undefined => true,
        // Strict equality with a Boolean literal cannot invoke JavaScript
        // coercion or allocate, even when the other operand is unknown.
        Expr::Compare {
            op: CompareOp::Eq,
            left,
            right,
        } => pure(left) && matches!(right.as_ref(), Expr::Bool(_)),
        Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => pure(left) && pure(right),
        _ => false,
    }
}
fn same(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::LocalGet(a), Expr::LocalGet(b)) => a == b,
        (Expr::Bool(a), Expr::Bool(b)) => a == b,
        (Expr::Undefined, Expr::Undefined) => true,
        (
            Expr::Compare {
                op: CompareOp::Eq,
                left: al,
                right: ar,
            },
            Expr::Compare {
                op: CompareOp::Eq,
                left: bl,
                right: br,
            },
        ) => same(al, bl) && same(ar, br),
        _ => false,
    }
}
fn runtime<'a>(expr: &'a Expr, name: &str) -> Option<&'a [Expr]> {
    match expr {
        Expr::NativeMethodCall {
            module,
            class_name: None,
            object: None,
            method,
            args,
        } if module == "__perry_runtime" && method == name => Some(args),
        _ => None,
    }
}
fn mode(expr: &Expr) -> bool {
    matches!(expr, Expr::Compare { op: CompareOp::Eq, left, right }
        if matches!(left.as_ref(), Expr::LocalGet(_)) && matches!(right.as_ref(), Expr::Bool(true)))
}
pub(crate) fn finish_args(
    condition: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Option<Vec<Expr>> {
    if !mode(condition) {
        return None;
    }
    let array = runtime(else_expr, "arrayRecordClose")?;
    if array.len() != 5 || !array.iter().all(pure) {
        return None;
    }
    let throwing = matches!(array[4], Expr::Bool(true));
    if !matches!(array[4], Expr::Bool(_)) {
        return None;
    }
    let protocol = runtime(
        then_expr,
        if throwing {
            "iteratorCloseOnThrow"
        } else {
            "iteratorCloseIfNotDone"
        },
    )?;
    if protocol.len() != if throwing { 3 } else { 2 }
        || !protocol.iter().all(pure)
        || !same(&protocol[1], &array[2])
        || (throwing && !same(&protocol[2], &array[3]))
    {
        return None;
    }
    let Expr::Compare {
        left: protocol_mode,
        ..
    } = condition
    else {
        unreachable!()
    };
    // The shared callee consumes exactly the original strict-true predicate.
    // Passing the binding avoids boxing and testing the predicate twice.
    Some(vec![
        protocol_mode.as_ref().clone(),
        array[0].clone(),
        array[1].clone(),
        protocol[0].clone(),
        array[2].clone(),
        array[3].clone(),
        array[4].clone(),
    ])
}
fn finish(expr: &Expr) -> Option<Vec<Expr>> {
    let Expr::Conditional {
        condition,
        then_expr,
        else_expr,
    } = expr
    else {
        return None;
    };
    finish_args(condition, then_expr, else_expr)
}
fn state_test(expr: &Expr, op: CompareOp, value: f64) -> Option<u32> {
    let Expr::Compare {
        op: actual,
        left,
        right,
    } = expr
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expr::LocalGet(id), Expr::Number(n)) if *actual == op && *n == value => Some(*id),
        _ => None,
    }
}
fn sets_state(stmt: &Stmt, state: u32) -> bool {
    matches!(stmt, Stmt::Expr(Expr::LocalSet(id, value))
        if *id == state && matches!(value.as_ref(), Expr::Number(2.0)))
}

/// Recognize only the complete compiler-generated handler. User catch/finally
/// code continues through ordinary try lowering.
fn exit_flag(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::LocalGet(id) => Some(*id),
        Expr::Compare {
            op: CompareOp::Eq,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Bool(true)) => {
            if let Expr::LocalGet(id) = left.as_ref() {
                Some(*id)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn abrupt_args(catch: &perry_hir::CatchClause, finally: &[Stmt]) -> Option<Vec<Expr>> {
    let (error, name) = catch.param.as_ref()?;
    // finally_inline wraps both arms with one compiler-owned exit flag.
    // Unwrap only the exact paired guard, and carry an already-exited
    // completion as state 2 so its original exception bypasses close.
    if let [Stmt::If {
        condition: flag_test,
        then_branch,
        else_branch: Some(body),
    }] = catch.body.as_slice()
    {
        let flag = exit_flag(flag_test)?;
        if !matches!(then_branch.as_slice(), [Stmt::Throw(Expr::LocalGet(id))] if id == error) {
            return None;
        }
        let [Stmt::If {
            condition:
                Expr::Unary {
                    op: perry_hir::UnaryOp::Not,
                    operand,
                },
            then_branch: once,
            else_branch: None,
        }] = finally
        else {
            return None;
        };
        if exit_flag(operand) != Some(flag) || !same(operand, flag_test) {
            return None;
        }
        let [Stmt::Expr(Expr::LocalSet(id, value)), cleanup @ ..] = once.as_slice() else {
            return None;
        };
        if *id != flag || !matches!(value.as_ref(), Expr::Bool(true)) {
            return None;
        }
        let plain = perry_hir::CatchClause {
            param: catch.param.clone(),
            body: body.clone(),
        };
        let mut args = abrupt_args(&plain, cleanup)?;
        args[4] = Expr::Conditional {
            condition: Box::new(flag_test.clone()),
            then_expr: Box::new(Expr::Number(2.0)),
            else_expr: Box::new(args[4].clone()),
        };
        return Some(args);
    }
    if !name.starts_with("__forof_err_") {
        return None;
    }
    let [Stmt::If {
        condition,
        then_branch,
        else_branch: None,
    }, Stmt::Throw(Expr::LocalGet(again))] = catch.body.as_slice()
    else {
        return None;
    };
    if again != error {
        return None;
    }
    let state = state_test(condition, CompareOp::Ne, 2.0)?;
    let [mark, Stmt::Throw(close)] = then_branch.as_slice() else {
        return None;
    };
    if !sets_state(mark, state) {
        return None;
    }
    let args = finish(close)?;
    if !same(&args[4], &Expr::Bool(false))
        || !same(&args[5], &Expr::LocalGet(*error))
        || !same(&args[6], &Expr::Bool(true))
    {
        return None;
    }
    let [Stmt::If {
        condition: normal,
        then_branch: normal_body,
        else_branch: None,
    }, Stmt::If {
        condition: release,
        then_branch: releases,
        else_branch: None,
    }] = finally
    else {
        return None;
    };
    if state_test(normal, CompareOp::Eq, 1.0) != Some(state)
        || state_test(release, CompareOp::Ne, 0.0) != Some(state)
    {
        return None;
    }
    let [mark, Stmt::Expr(normal_close)] = normal_body.as_slice() else {
        return None;
    };
    if !sets_state(mark, state) || finish(normal_close).is_none() {
        return None;
    }
    if !releases.iter().all(|stmt| matches!(stmt, Stmt::Expr(Expr::LocalSet(_, v)) if matches!(v.as_ref(), Expr::Undefined))) {return None;}
    let mut out = vec![
        args[0].clone(),
        args[1].clone(),
        args[2].clone(),
        args[3].clone(),
        Expr::LocalGet(state),
    ];
    out.extend(releases.iter().map(|s| match s {
        Stmt::Expr(e) => e.clone(),
        _ => unreachable!(),
    }));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_hir::{types::Type, CatchClause};

    fn rt(name: &str, args: Vec<Expr>) -> Expr {
        Expr::NativeMethodCall {
            module: "__perry_runtime".into(),
            class_name: None,
            object: None,
            method: name.into(),
            args,
        }
    }
    fn test_state(op: CompareOp, n: f64) -> Expr {
        Expr::Compare {
            op,
            left: Box::new(Expr::LocalGet(4)),
            right: Box::new(Expr::Number(n)),
        }
    }
    fn close(throwing: bool) -> Expr {
        let mut protocol = vec![Expr::LocalGet(3), Expr::Bool(false)];
        if throwing {
            protocol.push(Expr::LocalGet(8));
        }
        Expr::Conditional {
            condition: Box::new(Expr::Compare {
                op: CompareOp::Eq,
                left: Box::new(Expr::LocalGet(2)),
                right: Box::new(Expr::Bool(true)),
            }),
            then_expr: Box::new(rt(
                if throwing {
                    "iteratorCloseOnThrow"
                } else {
                    "iteratorCloseIfNotDone"
                },
                protocol,
            )),
            else_expr: Box::new(rt(
                "arrayRecordClose",
                vec![
                    Expr::LocalGet(1),
                    Expr::LocalGet(5),
                    Expr::Bool(false),
                    if throwing {
                        Expr::LocalGet(8)
                    } else {
                        Expr::Undefined
                    },
                    Expr::Bool(throwing),
                ],
            )),
        }
    }
    fn handler() -> (CatchClause, Vec<Stmt>) {
        let mark = Stmt::Expr(Expr::LocalSet(4, Box::new(Expr::Number(2.0))));
        let catch = CatchClause {
            param: Some((8, "__forof_err_8".into())),
            body: vec![
                Stmt::If {
                    condition: test_state(CompareOp::Ne, 2.0),
                    then_branch: vec![mark.clone(), Stmt::Throw(close(true))],
                    else_branch: None,
                },
                Stmt::Throw(Expr::LocalGet(8)),
            ],
        };
        let finally = vec![
            Stmt::If {
                condition: test_state(CompareOp::Eq, 1.0),
                then_branch: vec![mark, Stmt::Expr(close(false))],
                else_branch: None,
            },
            Stmt::If {
                condition: test_state(CompareOp::Ne, 0.0),
                then_branch: [1, 3, 6, 7]
                    .into_iter()
                    .map(|id| Stmt::Expr(Expr::LocalSet(id, Box::new(Expr::Undefined))))
                    .collect(),
                else_branch: None,
            },
        ];
        (catch, finally)
    }
    #[test]
    fn array_record_completion_is_one_outlined_abrupt_call() {
        crate::temp_root_coverage::under_both_lowerings(|mode| {
            let (catch, finally) = handler();
            let flag = 99;
            let catch = CatchClause {
                param: catch.param,
                body: vec![Stmt::If {
                    condition: Expr::LocalGet(flag),
                    then_branch: vec![Stmt::Throw(Expr::LocalGet(8))],
                    else_branch: Some(catch.body),
                }],
            };
            let finally = vec![Stmt::If {
                condition: Expr::Unary {
                    op: perry_hir::UnaryOp::Not,
                    operand: Box::new(Expr::LocalGet(flag)),
                },
                then_branch: std::iter::once(Stmt::Expr(Expr::LocalSet(
                    flag,
                    Box::new(Expr::Bool(true)),
                )))
                .chain(finally)
                .collect(),
                else_branch: None,
            }];
            let mut body = vec![Stmt::Let {
                id: flag,
                name: "__finally_exited_99".into(),
                ty: Type::Boolean,
                mutable: true,
                init: Some(Expr::Bool(false)),
            }];
            for (id, ty, init) in [
                (
                    1,
                    Type::Any,
                    Expr::Array(vec![Expr::String("source".into())]),
                ),
                (2, Type::Boolean, Expr::Bool(false)),
                (3, Type::Any, Expr::Undefined),
                (4, Type::Number, Expr::Number(0.0)),
                (5, Type::Number, Expr::Number(0.0)),
                (6, Type::Any, Expr::Undefined),
                (7, Type::Any, Expr::Undefined),
            ] {
                body.push(Stmt::Let {
                    id,
                    name: format!("record_{id}"),
                    ty,
                    mutable: true,
                    init: Some(init),
                });
            }
            body.push(Stmt::Try {
                body: vec![crate::temp_root_coverage::console_log(vec![Expr::String(
                    "body once".into(),
                )])],
                catch: Some(catch),
                finally: Some(finally),
            });
            let ir = crate::temp_root_coverage::main_ir_for("outlined_record", body);
            assert_eq!(
                ir.lines()
                    .filter(|l| l.contains("call void @js_array_record_abrupt("))
                    .count(),
                1,
                "{mode}: {ir}"
            );
            assert!(
                ir.lines()
                    .any(|line| line.contains("call double @js_array_record_finish(")),
                "{mode}: normal completion must also share the protocol: {ir}"
            );
            assert!(
                !ir.lines().any(|l| l.contains("call void @js_eh_try_push(")),
                "{mode}: no nested cleanup savepoint: {ir}"
            );
            assert!(
                !ir.lines()
                    .any(|l| l.contains("call double @js_get_exception(")),
                "{mode}: exception capture is shared: {ir}"
            );
            assert!(
                ir.contains("landingpad"),
                "{mode}: the unwind edge remains: {ir}"
            );
        });
    }
    #[test]
    fn array_record_completion_rejects_user_finally_and_effectful_operands() {
        let (catch, mut finally) = handler();
        assert!(abrupt_args(&catch, &finally).is_some());
        finally.push(Stmt::Expr(rt("arrayRecordCloseAbsent", vec![])));
        assert!(abrupt_args(&catch, &finally).is_none());
        let Expr::Conditional {
            condition,
            then_expr,
            mut else_expr,
        } = close(false)
        else {
            unreachable!()
        };
        let Expr::NativeMethodCall { args, .. } = else_expr.as_mut() else {
            unreachable!()
        };
        args[0] = rt("arrayRecordLiteral", vec![Expr::Number(1.0)]);
        assert!(
            finish_args(&condition, &then_expr, &else_expr).is_none(),
            "literal materialization must remain lazy"
        );
    }
}
