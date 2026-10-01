use super::uses_this::*;
use crate::ir::*;
use crate::types::Type;

#[test]
fn statement_expression_positions_preserve_lexical_bindings() {
    for expr in [Expr::This, Expr::NewTarget] {
        let leaf = || vec![Stmt::Expr(expr.clone())];
        let cases = vec![
            Stmt::Let {
                id: 0,
                name: "x".into(),
                ty: Type::Any,
                mutable: true,
                init: Some(expr.clone()),
            },
            Stmt::Return(Some(expr.clone())),
            Stmt::Throw(expr.clone()),
            Stmt::If {
                condition: expr.clone(),
                then_branch: vec![],
                else_branch: None,
            },
            Stmt::If {
                condition: Expr::Bool(false),
                then_branch: leaf(),
                else_branch: None,
            },
            Stmt::If {
                condition: Expr::Bool(false),
                then_branch: vec![],
                else_branch: Some(leaf()),
            },
            Stmt::While {
                condition: expr.clone(),
                body: vec![],
            },
            Stmt::While {
                condition: Expr::Bool(false),
                body: leaf(),
            },
            Stmt::DoWhile {
                condition: expr.clone(),
                body: vec![],
            },
            Stmt::DoWhile {
                condition: Expr::Bool(false),
                body: leaf(),
            },
            Stmt::For {
                init: Some(Box::new(Stmt::Expr(expr.clone()))),
                condition: None,
                update: None,
                body: vec![],
            },
            Stmt::For {
                init: None,
                condition: Some(expr.clone()),
                update: None,
                body: vec![],
            },
            Stmt::For {
                init: None,
                condition: None,
                update: Some(expr.clone()),
                body: vec![],
            },
            Stmt::For {
                init: None,
                condition: None,
                update: None,
                body: leaf(),
            },
            Stmt::Labeled {
                label: "outer".into(),
                body: Box::new(Stmt::DoWhile {
                    condition: Expr::Bool(false),
                    body: leaf(),
                }),
            },
            Stmt::Try {
                body: leaf(),
                catch: None,
                finally: None,
            },
            Stmt::Try {
                body: vec![],
                catch: Some(CatchClause {
                    param: None,
                    body: leaf(),
                }),
                finally: None,
            },
            Stmt::Try {
                body: vec![],
                catch: None,
                finally: Some(leaf()),
            },
            Stmt::Switch {
                discriminant: expr.clone(),
                cases: vec![],
            },
            Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: Some(expr.clone()),
                    body: vec![],
                }],
            },
            Stmt::Switch {
                discriminant: Expr::Number(0.0),
                cases: vec![SwitchCase {
                    test: None,
                    body: leaf(),
                }],
            },
        ];
        for stmt in cases {
            assert_eq!(
                uses_this_stmt(&stmt),
                matches!(expr, Expr::This),
                "{stmt:?}"
            );
            assert_eq!(
                uses_new_target_stmt(&stmt),
                matches!(expr, Expr::NewTarget),
                "{stmt:?}"
            );
        }
    }
}

#[test]
fn statement_leaves_have_no_lexical_binding_dependency() {
    for stmt in [
        Stmt::Return(None),
        Stmt::Break,
        Stmt::Continue,
        Stmt::LabeledBreak("outer".into()),
        Stmt::LabeledContinue("outer".into()),
        Stmt::PreallocateBoxes(vec![]),
        Stmt::PreallocateTdzBoxes(vec![]),
        Stmt::ReleaseBoxes(vec![]),
        Stmt::Let {
            id: 0,
            name: "x".into(),
            ty: Type::Any,
            mutable: true,
            init: None,
        },
        Stmt::DoWhile {
            condition: Expr::Bool(false),
            body: vec![Stmt::Continue],
        },
    ] {
        assert!(!uses_this_stmt(&stmt), "{stmt:?}");
        assert!(!uses_new_target_stmt(&stmt), "{stmt:?}");
    }
}

#[test]
fn nested_closures_propagate_only_captured_bindings() {
    for captures_this in [false, true] {
        for captures_new_target in [false, true] {
            let nested = Expr::Closure {
                func_id: 99,
                params: vec![],
                return_type: Type::Any,
                // The flags, rather than the body's contents, define scope boundaries.
                body: vec![Stmt::Expr(Expr::This), Stmt::Expr(Expr::NewTarget)],
                captures: vec![],
                mutable_captures: vec![],
                captures_this,
                captures_new_target,
                enclosing_class: None,
                is_arrow: captures_this || captures_new_target,
                is_async: false,
                is_generator: false,
                is_strict: false,
            };
            let body = vec![Stmt::Labeled {
                label: "outer".into(),
                body: Box::new(Stmt::DoWhile {
                    body: vec![Stmt::Expr(nested)],
                    condition: Expr::Bool(false),
                }),
            }];
            assert_eq!(closure_uses_this(&body), captures_this);
            assert_eq!(closure_uses_new_target(&body), captures_new_target);
        }
    }
}
