//! Preserve a generator's lexical private-name environment across suspension.
use super::*;
use crate::closure_local_inline::{for_each_expr_in_stmt_mut, nested_stmt_lists};
use perry_hir::walker::walk_expr_children_mut;

fn visit_body(body: &mut [Stmt], f: &mut dyn FnMut(&mut Expr)) {
    for stmt in body {
        for_each_expr_in_stmt_mut(stmt, &mut |expr| visit_expr(expr, f));
        for nested in nested_stmt_lists(stmt) {
            visit_body(nested, f);
        }
    }
}

fn visit_expr(expr: &mut Expr, f: &mut dyn FnMut(&mut Expr)) {
    walk_expr_children_mut(expr, &mut |child| visit_expr(child, f));
    if let Expr::Closure { body, .. } = expr {
        visit_body(body, f);
    }
    f(expr);
}

pub(super) fn uses_private_names(body: &mut [Stmt]) -> bool {
    let mut found = false;
    visit_body(body, &mut |expr| {
        found |= matches!(
            expr,
            Expr::PrivateGuard { .. } | Expr::PrivateBrandCheck { .. }
        );
    });
    found
}

pub(super) fn restore_in_continuations(
    body: &mut [Stmt],
    brand_id: LocalId,
    next_local_id: &mut u32,
) {
    visit_body(body, &mut |expr| {
        if let Expr::Closure { captures, body, .. } = expr {
            if captures.contains(&brand_id) {
                // Generator lowering runs after the source finally/return
                // normalization pass. Explicitly clean up normal returns too;
                // the finally arm below handles exceptions and fallthrough.
                for stmt in body.iter_mut() {
                    pop_before_return(stmt, next_local_id);
                }
                let resume = std::mem::take(body);
                *body = vec![
                    Stmt::Expr(Expr::PrivateLexicalBrandPush(Box::new(Expr::LocalGet(
                        brand_id,
                    )))),
                    Stmt::Try {
                        body: resume,
                        catch: None,
                        finally: Some(vec![Stmt::Expr(Expr::PrivateLexicalBrandPop)]),
                    },
                ];
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_return_cleanup(body: &mut Vec<Stmt>) {
        for index in 0..body.len() {
            if matches!(body[index], Stmt::Return(_)) {
                assert!(
                    index > 0
                        && matches!(&body[index - 1], Stmt::Expr(Expr::PrivateLexicalBrandPop)),
                    "normal returns must pop the environment, not bypass the finally"
                );
            }
            for nested in nested_stmt_lists(&mut body[index]) {
                assert_return_cleanup(nested);
            }
        }
    }

    #[test]
    fn private_generator_continuations_restore_captured_environment() {
        for is_async in [false, true] {
            let mut function = Function {
                id: 1,
                name: "gen".into(),
                type_params: vec![],
                params: vec![],
                return_type: Type::Any,
                body: vec![Stmt::Expr(Expr::Yield {
                    delegate: false,
                    value: Some(Box::new(Expr::PrivateGuard {
                        class_name: "C".into(),
                        class_id: 7,
                        field_name: "#v".into(),
                        kind: 0,
                        op: 0,
                        receiver_is_brand_owner: false,
                        object: Box::new(Expr::This),
                    })),
                })],
                is_strict: true,
                is_async,
                is_generator: true,
                is_exported: false,
                captures: vec![],
                decorators: vec![],
                was_plain_async: false,
                was_unrolled: false,
            };
            let mut local = 10;
            let mut func_id = 10;
            transform_generator_function(&mut function, &mut local, &mut func_id);
            let brand = function
                .body
                .iter()
                .find_map(|stmt| match stmt {
                    Stmt::Let {
                        id,
                        init: Some(Expr::PrivateLexicalBrand(_)),
                        ..
                    } => Some(*id),
                    _ => None,
                })
                .expect("capture lexical brand before the generator body starts");
            let mut continuations = 0;
            visit_body(&mut function.body, &mut |expr| {
                if let Expr::Closure { captures, body, .. } = expr {
                    if captures.contains(&brand) {
                        continuations += 1;
                        assert_return_cleanup(body);
                        assert!(matches!(
                            body.first(),
                            Some(Stmt::Expr(Expr::PrivateLexicalBrandPush(_)))
                        ));
                        assert!(matches!(&body[1], Stmt::Try { finally: Some(finally), .. }
                            if matches!(finally.as_slice(), [Stmt::Expr(Expr::PrivateLexicalBrandPop)])));
                    }
                }
            });
            assert!(
                continuations >= 3,
                "next/return/throw must all restore the brand"
            );
        }
    }
}

fn pop_before_return(stmt: &mut Stmt, next_local_id: &mut u32) {
    if let Stmt::Return(value) = stmt {
        let mut sequence = Vec::new();
        let result = value.take().map(|value| {
            let id = alloc_local(next_local_id);
            sequence.push(Stmt::Let {
                id,
                name: "__gen_private_result".into(),
                ty: Type::Any,
                mutable: false,
                init: Some(value),
            });
            Expr::LocalGet(id)
        });
        sequence.push(Stmt::Expr(Expr::PrivateLexicalBrandPop));
        sequence.push(Stmt::Return(result));
        *stmt = Stmt::If {
            condition: Expr::Bool(true),
            then_branch: sequence,
            else_branch: None,
        };
    } else if let Stmt::Labeled { body, .. } = stmt {
        pop_before_return(body, next_local_id);
    } else {
        for body in nested_stmt_lists(stmt) {
            for stmt in body {
                pop_before_return(stmt, next_local_id);
            }
        }
    }
}
