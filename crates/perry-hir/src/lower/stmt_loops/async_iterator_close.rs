//! `for await` cleanup belongs to the loop's lifetime, including a generator
//! suspended at a yield. Inserting `return()` before syntactic break/return
//! statements cannot close it when the consumer calls the generator's return.
use super::*;

pub(crate) fn emit_driver(
    ctx: &mut LoweringContext,
    output: &mut Vec<Stmt>,
    iter_id: LocalId,
    result_id: LocalId,
    next_call: Expr,
    mut body: Vec<Stmt>,
) {
    let active = boolean_local(ctx, output, "__iterator_close_active");
    let throwing = boolean_local(ctx, output, "__iterator_close_throwing");
    // IteratorValue failures do not close the iterator. Mark it active only
    // after reading value, but before the binding (including destructuring)
    // can throw. ScopedTemp preserves single evaluation without a heap box.
    let Some(Stmt::Let {
        init: Some(value), ..
    }) = body.first_mut()
    else {
        unreachable!("async iterator driver requires a value binding")
    };
    let value_id = ctx.fresh_local();
    *value = Expr::ScopedTemp {
        id: value_id,
        value: Box::new(std::mem::replace(value, Expr::Undefined)),
        body: Box::new(Expr::Sequence(vec![
            Expr::LocalSet(active, Box::new(Expr::Bool(true))),
            Expr::LocalGet(value_id),
        ])),
    };
    let mut driver = iter_driver_while_stmt(result_id, iterator_result_validated(next_call), body);
    let Stmt::While {
        body: loop_body, ..
    } = &mut driver
    else {
        unreachable!()
    };
    // A failed next()/done read and natural exhaustion must not close. Once
    // a value has arrived, any abrupt exit from its binding/body must close.
    loop_body.insert(0, set_bool(active, false));

    let method = ctx.fresh_local();
    ctx.locals
        .push((format!("__iterator_return_{method}"), method, Type::Any));
    let close = vec![
        Stmt::Let {
            id: method,
            name: format!("__iterator_return_{method}"),
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(iter_id)),
                property: "return".to_string(),
            }),
        },
        Stmt::If {
            condition: Expr::Compare {
                op: CompareOp::LooseNe,
                left: Box::new(Expr::LocalGet(method)),
                right: Box::new(Expr::Null),
            },
            then_branch: vec![Stmt::Expr(iterator_result_validated(Expr::Await(
                Box::new(Expr::Call {
                    callee: Box::new(Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(Expr::LocalGet(method)),
                        property: "call".to_string(),
                    }),
                    args: vec![Expr::LocalGet(iter_id)],
                    type_args: vec![],
                    byte_offset: 0,
                }),
            )))],
            else_branch: None,
        },
    ];
    let error = ctx.fresh_local();
    let ignored = ctx.fresh_local();
    output.push(Stmt::Try {
        body: vec![driver],
        catch: Some(CatchClause {
            param: Some((error, format!("__iterator_error_{error}"))),
            body: vec![set_bool(throwing, true), Stmt::Throw(Expr::LocalGet(error))],
        }),
        finally: Some(vec![Stmt::If {
            condition: Expr::LocalGet(active),
            then_branch: vec![Stmt::If {
                condition: Expr::LocalGet(throwing),
                // A throw completion takes precedence over failure of close.
                then_branch: vec![Stmt::Try {
                    body: close.clone(),
                    catch: Some(CatchClause {
                        param: Some((ignored, format!("__iterator_ignored_{ignored}"))),
                        body: vec![],
                    }),
                    finally: None,
                }],
                else_branch: Some(close),
            }],
            else_branch: None,
        }]),
    });
}

fn set_bool(id: LocalId, value: bool) -> Stmt {
    Stmt::Expr(Expr::LocalSet(id, Box::new(Expr::Bool(value))))
}

fn boolean_local(ctx: &mut LoweringContext, output: &mut Vec<Stmt>, prefix: &str) -> LocalId {
    let id = ctx.fresh_local();
    let name = format!("{prefix}_{id}");
    ctx.locals.push((name.clone(), id, Type::Boolean));
    output.push(Stmt::Let {
        id,
        name,
        ty: Type::Boolean,
        mutable: true,
        init: Some(Expr::Bool(false)),
    });
    id
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_async_iterator_routes_have_a_finally_at_the_loop_boundary() {
        for loop_source in ["source", "values()"] {
            let source = format!(
                "declare const source: any; async function* values() {{ yield 1; }} \
                 async function* consume() {{ for await (const value of {loop_source}) {{ yield value; }} }}"
            );
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    let mut cache = perry_diagnostics::SourceCache::new();
                    let parsed =
                        perry_parser::parse_typescript_with_cache(&source, "close.ts", &mut cache)
                            .unwrap();
                    let module = crate::lower_module(&parsed.module, "test", "close.ts").unwrap();
                    let function = module
                        .functions
                        .iter()
                        .find(|f| f.name == "consume")
                        .unwrap();
                    assert!(
                        function.body.iter().any(|stmt| matches!(stmt,
                            crate::Stmt::Try { body, finally: Some(_), .. }
                            if matches!(body.first(), Some(crate::Stmt::While { .. }))
                        )),
                        "a suspended yield must remain protected by iterator cleanup"
                    );
                })
                .unwrap()
                .join()
                .unwrap();
        }
    }
}
