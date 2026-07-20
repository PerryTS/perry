//! Generator resume-body wrapping helpers (executing guard, rethrow, and
//! the `executing`-clear-before-return pass), split out of `lower.rs`.

use super::*;

pub(crate) fn wrap_generator_resume_body(
    mut body: Vec<Stmt>,
    executing_id: LocalId,
    done_id: LocalId,
    catch_id: LocalId,
    is_async_generator: bool,
) -> Vec<Stmt> {
    prepend_executing_clear_before_returns(&mut body, executing_id);
    if is_async_generator {
        wrap_returns_in_promise(&mut body);
    }

    vec![
        generator_executing_guard(executing_id, is_async_generator),
        Stmt::Try {
            body,
            catch: Some(CatchClause {
                param: Some((catch_id, "__gen_exec_e".to_string())),
                body: vec![
                    Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
                    Stmt::Expr(Expr::LocalSet(executing_id, Box::new(Expr::Bool(false)))),
                    generator_resume_rethrow(Expr::LocalGet(catch_id), is_async_generator),
                ],
            }),
            finally: None,
        },
    ]
}

pub(crate) fn generator_executing_guard(executing_id: LocalId, is_async_generator: bool) -> Stmt {
    Stmt::If {
        condition: Expr::LocalGet(executing_id),
        then_branch: vec![generator_resume_rethrow(
            generator_executing_type_error(),
            is_async_generator,
        )],
        else_branch: None,
    }
}

pub(crate) fn generator_resume_rethrow(value: Expr, is_async_generator: bool) -> Stmt {
    if is_async_generator {
        Stmt::Return(Some(promise_reject(value)))
    } else {
        Stmt::Throw(value)
    }
}

pub(crate) fn generator_executing_type_error() -> Expr {
    Expr::TypeErrorNew(Box::new(Expr::String(
        "Generator is already executing".to_string(),
    )))
}

pub(crate) fn promise_reject(value: Expr) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::GlobalGet(0)),
            property: "reject".to_string(),
        }),
        args: vec![value],
        type_args: vec![],
        byte_offset: 0,
    }
}

pub(crate) fn prepend_executing_clear_before_returns(stmts: &mut Vec<Stmt>, executing_id: LocalId) {
    let mut new_body: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in stmts.drain(..) {
        match &mut stmt {
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                prepend_executing_clear_before_returns(then_branch, executing_id);
                if let Some(else_branch) = else_branch {
                    prepend_executing_clear_before_returns(else_branch, executing_id);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::For { body, .. } => {
                prepend_executing_clear_before_returns(body, executing_id);
            }
            Stmt::Try {
                body,
                catch,
                finally,
            } => {
                prepend_executing_clear_before_returns(body, executing_id);
                if let Some(catch) = catch {
                    prepend_executing_clear_before_returns(&mut catch.body, executing_id);
                }
                if let Some(finally) = finally {
                    prepend_executing_clear_before_returns(finally, executing_id);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases.iter_mut() {
                    prepend_executing_clear_before_returns(&mut case.body, executing_id);
                }
            }
            Stmt::Labeled { body, .. } => {
                let mut wrapped = vec![std::mem::replace(body.as_mut(), Stmt::Break)];
                prepend_executing_clear_before_returns(&mut wrapped, executing_id);
                **body = wrapped.into_iter().next().unwrap();
            }
            _ => {}
        }
        if matches!(stmt, Stmt::Return(_)) {
            new_body.push(Stmt::Expr(Expr::LocalSet(
                executing_id,
                Box::new(Expr::Bool(false)),
            )));
        }
        new_body.push(stmt);
    }
    *stmts = new_body;
}
