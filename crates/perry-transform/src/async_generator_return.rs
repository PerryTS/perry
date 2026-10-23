//! `return E` in an async generator is `return ? Await(E)` (ECMAScript
//! ReturnStatement evaluation, step 3: "If GetGeneratorKind() is async, set
//! exprValue to ? Await(exprValue)"). The Await is part of the return
//! statement itself, so it runs inside every enclosing `try`: a returned
//! rejected promise throws into the generator's own `catch`, and a returned
//! promise or thenable completes the generator with its settled value.
//!
//! The desugar runs before `finally_inline`, which hoists a return operand
//! into a temp and marks the enclosing try exited before the `return`; the
//! await therefore lands in that temp's initializer, inside the try, exactly
//! like a user-written `return await E`. A bare `return;` takes no Await.
//! Nested closures are their own function scopes and are not descended.

use perry_hir::{Expr, Stmt};

pub(crate) fn await_returned_operands(stmts: &mut [Stmt]) {
    for stmt in stmts {
        match stmt {
            Stmt::Return(Some(expr)) => {
                if !matches!(expr, Expr::Await(_) | Expr::Yield { .. }) {
                    let operand = std::mem::replace(expr, Expr::Undefined);
                    *expr = Expr::Await(Box::new(operand));
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                await_returned_operands(then_branch);
                if let Some(eb) = else_branch {
                    await_returned_operands(eb);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                await_returned_operands(body)
            }
            Stmt::Labeled { body, .. } => await_returned_operands(std::slice::from_mut(body)),
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                await_returned_operands(body);
                if let Some(c) = catch {
                    await_returned_operands(&mut c.body);
                }
                if let Some(f) = finally {
                    await_returned_operands(f);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    await_returned_operands(&mut case.body);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_in_try_await_their_operand_and_bare_returns_do_not() {
        let mut body = vec![Stmt::Try {
            body: vec![Stmt::Return(Some(Expr::LocalGet(1)))],
            catch: Some(perry_hir::CatchClause {
                param: None,
                body: vec![Stmt::Return(None)],
            }),
            finally: None,
        }];
        await_returned_operands(&mut body);
        let Stmt::Try {
            body: inner,
            catch: Some(c),
            ..
        } = &body[0]
        else {
            panic!("try");
        };
        assert!(
            matches!(&inner[0], Stmt::Return(Some(Expr::Await(op))) if matches!(**op, Expr::LocalGet(1)))
        );
        assert!(matches!(&c.body[0], Stmt::Return(None)));
    }

    #[test]
    fn an_existing_await_is_not_awaited_twice() {
        let mut body = vec![Stmt::Return(Some(Expr::Await(Box::new(Expr::LocalGet(1)))))];
        await_returned_operands(&mut body);
        assert!(
            matches!(&body[0], Stmt::Return(Some(Expr::Await(op))) if matches!(**op, Expr::LocalGet(1)))
        );
    }
}
