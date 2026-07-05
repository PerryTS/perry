//! Lowering of destructuring assignment statements (e.g. `[a, b] = expr` as a statement).

use super::*;

#[derive(Clone)]
enum PreparedTarget {
    Local(LocalId),
    Property { object: Expr, property: String },
    Index { object: Expr, index: Expr },
    Array(ast::ArrayPat),
    Object(ast::ObjectPat),
    Skip,
}

fn fresh_destruct_local(ctx: &mut LoweringContext, prefix: &str, ty: Type) -> (LocalId, String) {
    let id = ctx.fresh_local();
    let name = format!("__{}_{}", prefix, id);
    ctx.locals.push((name.clone(), id, ty));
    (id, name)
}

fn runtime_iterator_call(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

fn extern_call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }),
        args,
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

pub(crate) fn lower_destructuring_assignment_stmt(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    rhs: &ast::Expr,
) -> Result<Vec<Stmt>> {
    let rhs_expr = lower_expr(ctx, rhs)?;
    let (tmp_id, tmp_name) = fresh_destruct_local(ctx, "destruct", Type::Any);

    let mut result = vec![Stmt::Let {
        id: tmp_id,
        name: tmp_name,
        ty: Type::Any,
        mutable: false,
        init: Some(rhs_expr),
    }];

    result.extend(lower_destructuring_assignment_stmt_from_local(
        ctx, pat, tmp_id,
    )?);
    Ok(result)
}

/// Helper for nested destructuring - assigns from an already-computed local.
pub(crate) fn lower_destructuring_assignment_stmt_from_local(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    source_id: LocalId,
) -> Result<Vec<Stmt>> {
    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            lower_array_assignment_from_expr(ctx, arr_pat, Expr::LocalGet(source_id))
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            lower_object_assignment_from_expr(ctx, obj_pat, Expr::LocalGet(source_id))
        }
        ast::AssignTargetPat::Invalid(_) => Err(anyhow!("Invalid assignment target pattern")),
    }
}

fn lower_array_assignment_from_expr(
    ctx: &mut LoweringContext,
    arr_pat: &ast::ArrayPat,
    source: Expr,
) -> Result<Vec<Stmt>> {
    let (iter_id, iter_name) = fresh_destruct_local(ctx, "destruct_iter", Type::Any);
    let (done_id, done_name) = fresh_destruct_local(ctx, "destruct_done", Type::Boolean);

    let mut result = vec![
        Stmt::Let {
            id: iter_id,
            name: iter_name,
            ty: Type::Any,
            mutable: false,
            init: Some(Expr::GetIterator(Box::new(source))),
        },
        Stmt::Let {
            id: done_id,
            name: done_name,
            ty: Type::Boolean,
            mutable: true,
            init: Some(Expr::Bool(false)),
        },
    ];

    let mut body = Vec::new();
    for elem in &arr_pat.elems {
        let (value_id, value_name) = fresh_destruct_local(ctx, "destruct_value", Type::Any);
        body.push(Stmt::Let {
            id: value_id,
            name: value_name,
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Undefined),
        });

        if let Some(elem_pat) = elem {
            let (prepare, target, default_value) = prepare_target_with_default(ctx, elem_pat)?;
            body.extend(prepare);
            body.extend(iterator_next_value_stmts(ctx, iter_id, done_id, value_id));
            let assigned = value_with_default(ctx, Expr::LocalGet(value_id), default_value)?;
            body.extend(assign_prepared_target(ctx, target, assigned)?);
        } else {
            body.extend(iterator_next_value_stmts(ctx, iter_id, done_id, value_id));
        }
    }

    let close_stmt = Stmt::Expr(runtime_iterator_call(
        "iteratorCloseIfNotDone",
        vec![Expr::LocalGet(iter_id), Expr::LocalGet(done_id)],
    ));
    let (exc_id, exc_name) = fresh_destruct_local(ctx, "destruct_error", Type::Any);
    result.push(Stmt::Try {
        body,
        catch: Some(CatchClause {
            param: Some((exc_id, exc_name)),
            body: vec![
                Stmt::Try {
                    body: vec![close_stmt.clone()],
                    catch: Some(CatchClause {
                        param: None,
                        body: Vec::new(),
                    }),
                    finally: None,
                },
                Stmt::Throw(Expr::LocalGet(exc_id)),
            ],
        }),
        finally: None,
    });
    result.push(close_stmt);

    Ok(result)
}

fn iterator_next_value_stmts(
    ctx: &mut LoweringContext,
    iter_id: LocalId,
    done_id: LocalId,
    value_id: LocalId,
) -> Vec<Stmt> {
    let (step_id, step_name) = fresh_destruct_local(ctx, "destruct_step", Type::Any);
    let pull_next = vec![
        Stmt::Let {
            id: step_id,
            name: step_name,
            ty: Type::Any,
            mutable: false,
            init: Some(runtime_iterator_call(
                "iteratorNextResult",
                vec![Expr::LocalGet(iter_id)],
            )),
        },
        Stmt::If {
            condition: Expr::PropertyGet {
                object: Box::new(Expr::LocalGet(step_id)),
                property: "done".to_string(),
            },
            then_branch: vec![
                Stmt::Expr(Expr::LocalSet(done_id, Box::new(Expr::Bool(true)))),
                Stmt::Expr(Expr::LocalSet(value_id, Box::new(Expr::Undefined))),
            ],
            else_branch: Some(vec![Stmt::Expr(Expr::LocalSet(
                value_id,
                Box::new(Expr::PropertyGet {
                    object: Box::new(Expr::LocalGet(step_id)),
                    property: "value".to_string(),
                }),
            ))]),
        },
    ];

    vec![Stmt::If {
        condition: Expr::LocalGet(done_id),
        then_branch: vec![Stmt::Expr(Expr::LocalSet(
            value_id,
            Box::new(Expr::Undefined),
        ))],
        else_branch: Some(pull_next),
    }]
}

fn lower_object_assignment_from_expr(
    ctx: &mut LoweringContext,
    obj_pat: &ast::ObjectPat,
    source: Expr,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    for prop in &obj_pat.props {
        match prop {
            ast::ObjectPatProp::KeyValue(kv) => {
                let (key_prepare, get_value) =
                    object_property_get_expr(ctx, &kv.key, source.clone())?;
                result.extend(key_prepare);

                let (prepare, target, default_value) = prepare_target_with_default(ctx, &kv.value)?;
                result.extend(prepare);

                let (value_id, value_name) = fresh_destruct_local(ctx, "destruct_value", Type::Any);
                result.push(Stmt::Let {
                    id: value_id,
                    name: value_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(get_value),
                });
                let assigned = value_with_default(ctx, Expr::LocalGet(value_id), default_value)?;
                result.extend(assign_prepared_target(ctx, target, assigned)?);
            }
            ast::ObjectPatProp::Assign(assign) => {
                let name = assign.key.sym.to_string();
                let (value_id, value_name) = fresh_destruct_local(ctx, "destruct_value", Type::Any);
                result.push(Stmt::Let {
                    id: value_id,
                    name: value_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::PropertyGet {
                        object: Box::new(source.clone()),
                        property: name.clone(),
                    }),
                });

                if let Some(id) = ctx.lookup_local(&name) {
                    result.push(Stmt::Expr(Expr::LocalSet(
                        id,
                        Box::new(Expr::LocalGet(value_id)),
                    )));
                } else {
                    return Err(anyhow!(
                        "Assignment to undeclared variable in destructuring: {}",
                        name
                    ));
                }
            }
            ast::ObjectPatProp::Rest(rest) => {
                // `{ ...rest } = source` → rest = ObjectRest(source, [named keys]).
                // Previously skipped (same gap as assignment_expr.rs), leaving the
                // rest binding undefined and collapsing spread-through props.
                let exclude_keys =
                    super::helpers::collect_static_object_pattern_keys(&obj_pat.props);
                let rest_expr = Expr::ObjectRest {
                    object: Box::new(source.clone()),
                    exclude_keys,
                };
                // Bind through the general target machinery so both an identifier
                // and a member-expression target (`({ ...a.b } = q)`) work — the
                // same path the KeyValue arm above uses. Rest patterns carry no
                // default.
                let (prepare, target, _default) = prepare_target_with_default(ctx, &rest.arg)?;
                result.extend(prepare);
                result.extend(assign_prepared_target(ctx, target, rest_expr)?);
            }
        }
    }

    Ok(result)
}

fn object_property_get_expr(
    ctx: &mut LoweringContext,
    key: &ast::PropName,
    source: Expr,
) -> Result<(Vec<Stmt>, Expr)> {
    match key {
        ast::PropName::Ident(ident) => Ok((
            Vec::new(),
            Expr::PropertyGet {
                object: Box::new(source),
                property: ident.sym.to_string(),
            },
        )),
        ast::PropName::Str(s) => Ok((
            Vec::new(),
            Expr::PropertyGet {
                object: Box::new(source),
                property: s.value.as_str().unwrap_or("").to_string(),
            },
        )),
        ast::PropName::Num(n) => Ok((
            Vec::new(),
            Expr::PropertyGet {
                object: Box::new(source),
                property: n.value.to_string(),
            },
        )),
        ast::PropName::Computed(computed) => {
            let key_expr = extern_call(
                "js_object_literal_to_property_key",
                vec![lower_expr(ctx, &computed.expr)?],
            );
            let (key_id, key_name) = fresh_destruct_local(ctx, "destruct_key", Type::Any);
            Ok((
                vec![Stmt::Let {
                    id: key_id,
                    name: key_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(key_expr),
                }],
                Expr::IndexGet {
                    object: Box::new(source),
                    index: Box::new(Expr::LocalGet(key_id)),
                },
            ))
        }
        _ => Ok((Vec::new(), Expr::Undefined)),
    }
}

fn prepare_target_with_default(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
) -> Result<(Vec<Stmt>, PreparedTarget, Option<Expr>)> {
    if let ast::Pat::Assign(assign_pat) = pat {
        let (prepare, target, _) = prepare_assignment_target(ctx, &assign_pat.left)?;
        let default_value = lower_expr(ctx, &assign_pat.right)?;
        return Ok((prepare, target, Some(default_value)));
    }
    let (prepare, target, default_value) = prepare_assignment_target(ctx, pat)?;
    Ok((prepare, target, default_value))
}

fn prepare_assignment_target(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
) -> Result<(Vec<Stmt>, PreparedTarget, Option<Expr>)> {
    match pat {
        ast::Pat::Ident(ident) => {
            let name = ident.id.sym.to_string();
            if let Some(id) = ctx.lookup_local(&name) {
                Ok((Vec::new(), PreparedTarget::Local(id), None))
            } else {
                Err(anyhow!(
                    "Assignment to undeclared variable in destructuring: {}",
                    name
                ))
            }
        }
        ast::Pat::Array(nested_arr) => {
            Ok((Vec::new(), PreparedTarget::Array(nested_arr.clone()), None))
        }
        ast::Pat::Object(nested_obj) => {
            Ok((Vec::new(), PreparedTarget::Object(nested_obj.clone()), None))
        }
        ast::Pat::Expr(inner_expr) => match inner_expr.as_ref() {
            ast::Expr::Member(member) => {
                let mut prepare = Vec::new();
                let (object_id, object_name) =
                    fresh_destruct_local(ctx, "destruct_target", Type::Any);
                prepare.push(Stmt::Let {
                    id: object_id,
                    name: object_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(lower_expr(ctx, &member.obj)?),
                });
                match &member.prop {
                    ast::MemberProp::Ident(prop_ident) => Ok((
                        prepare,
                        PreparedTarget::Property {
                            object: Expr::LocalGet(object_id),
                            property: prop_ident.sym.to_string(),
                        },
                        None,
                    )),
                    ast::MemberProp::Computed(computed) => {
                        let (key_id, key_name) =
                            fresh_destruct_local(ctx, "destruct_target_key", Type::Any);
                        prepare.push(Stmt::Let {
                            id: key_id,
                            name: key_name,
                            ty: Type::Any,
                            mutable: false,
                            init: Some(lower_expr(ctx, &computed.expr)?),
                        });
                        Ok((
                            prepare,
                            PreparedTarget::Index {
                                object: Expr::LocalGet(object_id),
                                index: Expr::LocalGet(key_id),
                            },
                            None,
                        ))
                    }
                    // Private-field assignment target, e.g.
                    // `[this.#field] = [v]` or `({a: this.#field} = src)`. Brand
                    // -guard the receiver so a write to a wrong receiver (or to
                    // a getter-only accessor / private method) throws TypeError,
                    // matching `this.#field = v`.
                    ast::MemberProp::PrivateName(private) => {
                        let property = format!("#{}", private.name);
                        let guarded = crate::lower::wrap_private_guard(
                            ctx,
                            Box::new(Expr::LocalGet(object_id)),
                            &property,
                            crate::lower::PRIV_OP_WRITE,
                        );
                        Ok((
                            prepare,
                            PreparedTarget::Property {
                                object: *guarded,
                                property,
                            },
                            None,
                        ))
                    }
                    _ => Err(anyhow!(
                        "Unsupported member expression in destructuring assignment"
                    )),
                }
            }
            _ => Err(anyhow!(
                "Unsupported expression pattern in destructuring assignment"
            )),
        },
        ast::Pat::Rest(_) => Ok((Vec::new(), PreparedTarget::Skip, None)),
        _ => Ok((Vec::new(), PreparedTarget::Skip, None)),
    }
}

fn value_with_default(
    _ctx: &mut LoweringContext,
    value: Expr,
    default_value: Option<Expr>,
) -> Result<Expr> {
    let Some(default_value) = default_value else {
        return Ok(value);
    };

    Ok(Expr::Conditional {
        condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(value.clone()))),
        then_expr: Box::new(default_value),
        else_expr: Box::new(value),
    })
}

fn assign_prepared_target(
    ctx: &mut LoweringContext,
    target: PreparedTarget,
    value: Expr,
) -> Result<Vec<Stmt>> {
    match target {
        PreparedTarget::Local(id) => Ok(vec![Stmt::Expr(Expr::LocalSet(id, Box::new(value)))]),
        PreparedTarget::Property { object, property } => Ok(vec![Stmt::Expr(Expr::PutValueSet {
            target: Box::new(object.clone()),
            key: Box::new(Expr::String(property)),
            value: Box::new(value),
            receiver: Box::new(object),
            strict: ctx.current_strict,
        })]),
        PreparedTarget::Index { object, index } => Ok(vec![Stmt::Expr(Expr::PutValueSet {
            target: Box::new(object.clone()),
            key: Box::new(index),
            value: Box::new(value),
            receiver: Box::new(object),
            strict: ctx.current_strict,
        })]),
        PreparedTarget::Array(arr) => lower_array_assignment_from_expr(ctx, &arr, value),
        PreparedTarget::Object(obj) => lower_object_assignment_from_expr(ctx, &obj, value),
        PreparedTarget::Skip => Ok(Vec::new()),
    }
}
