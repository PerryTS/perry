//! Lowering of destructuring assignment statements (e.g. `[a, b] = expr` as a statement).

use super::*;

pub(crate) fn lower_destructuring_assignment_stmt(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    rhs: &ast::Expr,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    // First, evaluate and store the RHS in a temporary variable
    let rhs_expr = lower_expr(ctx, rhs)?;
    let tmp_id = ctx.fresh_local();
    let tmp_name = format!("__destruct_{}", tmp_id);
    let tmp_ty = Type::Any; // Could infer from rhs, but Any is safe
    ctx.locals.push((tmp_name.clone(), tmp_id, tmp_ty.clone()));

    result.push(Stmt::Let {
        id: tmp_id,
        name: tmp_name,
        ty: tmp_ty,
        mutable: false,
        init: Some(rhs_expr),
    });

    // Now generate assignments from the temp
    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                if let Some(elem_pat) = elem {
                    let index_expr = Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(tmp_id)),
                        index: Box::new(Expr::Number(idx as f64)),
                    };

                    match elem_pat {
                        ast::Pat::Ident(ident) => {
                            let name = ident.id.sym.to_string();
                            if let Some(id) = ctx.lookup_local(&name) {
                                result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(index_expr))));
                            } else {
                                return Err(anyhow!(
                                    "Assignment to undeclared variable in destructuring: {}",
                                    name
                                ));
                            }
                        }
                        ast::Pat::Array(nested_arr) => {
                            // Nested array destructuring
                            // First create a temp for this element
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            // Then recursively assign from it
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Array(nested_arr.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Object(nested_obj) => {
                            // Nested object destructuring
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Object(nested_obj.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Expr(inner_expr) => {
                            // Expression pattern like [obj.prop, obj2.prop2] = arr
                            match inner_expr.as_ref() {
                                ast::Expr::Member(member) => {
                                    let object = Box::new(lower_expr(ctx, &member.obj)?);
                                    match &member.prop {
                                        ast::MemberProp::Ident(prop_ident) => {
                                            let property = prop_ident.sym.to_string();
                                            result.push(Stmt::Expr(Expr::PropertySet {
                                                object,
                                                property,
                                                value: Box::new(index_expr),
                                            }));
                                        }
                                        ast::MemberProp::Computed(computed) => {
                                            let index = Box::new(lower_expr(ctx, &computed.expr)?);
                                            result.push(Stmt::Expr(Expr::IndexSet {
                                                object,
                                                index,
                                                value: Box::new(index_expr),
                                            }));
                                        }
                                        _ => {
                                            return Err(anyhow!(
                                                "Unsupported member expression in destructuring assignment"
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    return Err(anyhow!(
                                        "Unsupported expression pattern in destructuring assignment"
                                    ));
                                }
                            }
                        }
                        _ => {
                            // Other patterns (Rest, etc.) - skip for now
                        }
                    }
                }
            }
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            ast::PropName::Num(n) => n.value.to_string(),
                            _ => continue,
                        };

                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: key,
                        };

                        match &*kv.value {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    result
                                        .push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring: {}",
                                        name
                                    ));
                                }
                            }
                            ast::Pat::Array(nested_arr) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Array(nested_arr.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            ast::Pat::Object(nested_obj) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Object(nested_obj.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            _ => {}
                        }
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            property: name.clone(),
                        };

                        if let Some(id) = ctx.lookup_local(&name) {
                            result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                        } else {
                            return Err(anyhow!(
                                "Assignment to undeclared variable in destructuring: {}",
                                name
                            ));
                        }
                    }
                    ast::ObjectPatProp::Rest(_) => {
                        // Rest pattern - skip for now
                    }
                }
            }
        }
        ast::AssignTargetPat::Invalid(_) => {
            return Err(anyhow!("Invalid assignment target pattern"));
        }
    }

    Ok(result)
}

/// Helper for nested destructuring - assigns from an already-computed local
pub(crate) fn lower_destructuring_assignment_stmt_from_local(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    source_id: LocalId,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();

    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                if let Some(elem_pat) = elem {
                    let index_expr = Expr::IndexGet {
                        object: Box::new(Expr::LocalGet(source_id)),
                        index: Box::new(Expr::Number(idx as f64)),
                    };

                    match elem_pat {
                        ast::Pat::Ident(ident) => {
                            let name = ident.id.sym.to_string();
                            if let Some(id) = ctx.lookup_local(&name) {
                                result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(index_expr))));
                            } else {
                                return Err(anyhow!(
                                    "Assignment to undeclared variable in destructuring: {}",
                                    name
                                ));
                            }
                        }
                        ast::Pat::Array(nested_arr) => {
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Array(nested_arr.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Object(nested_obj) => {
                            let nested_tmp_id = ctx.fresh_local();
                            let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                            ctx.locals
                                .push((nested_tmp_name.clone(), nested_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: nested_tmp_id,
                                name: nested_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(index_expr),
                            });
                            let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                ctx,
                                &ast::AssignTargetPat::Object(nested_obj.clone()),
                                nested_tmp_id,
                            )?;
                            result.extend(nested_stmts);
                        }
                        ast::Pat::Expr(inner_expr) => match inner_expr.as_ref() {
                            ast::Expr::Member(member) => {
                                let object = Box::new(lower_expr(ctx, &member.obj)?);
                                match &member.prop {
                                    ast::MemberProp::Ident(prop_ident) => {
                                        let property = prop_ident.sym.to_string();
                                        result.push(Stmt::Expr(Expr::PropertySet {
                                            object,
                                            property,
                                            value: Box::new(index_expr),
                                        }));
                                    }
                                    ast::MemberProp::Computed(computed) => {
                                        let index = Box::new(lower_expr(ctx, &computed.expr)?);
                                        result.push(Stmt::Expr(Expr::IndexSet {
                                            object,
                                            index,
                                            value: Box::new(index_expr),
                                        }));
                                    }
                                    _ => {
                                        return Err(anyhow!(
                                                "Unsupported member expression in destructuring assignment"
                                            ));
                                    }
                                }
                            }
                            _ => {
                                return Err(anyhow!(
                                    "Unsupported expression pattern in destructuring assignment"
                                ));
                            }
                        },
                        _ => {}
                    }
                }
            }
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key = match &kv.key {
                            ast::PropName::Ident(ident) => ident.sym.to_string(),
                            ast::PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                            ast::PropName::Num(n) => n.value.to_string(),
                            _ => continue,
                        };

                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(source_id)),
                            property: key,
                        };

                        match &*kv.value {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    result
                                        .push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring: {}",
                                        name
                                    ));
                                }
                            }
                            ast::Pat::Array(nested_arr) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Array(nested_arr.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            ast::Pat::Object(nested_obj) => {
                                let nested_tmp_id = ctx.fresh_local();
                                let nested_tmp_name = format!("__destruct_{}", nested_tmp_id);
                                ctx.locals.push((
                                    nested_tmp_name.clone(),
                                    nested_tmp_id,
                                    Type::Any,
                                ));
                                result.push(Stmt::Let {
                                    id: nested_tmp_id,
                                    name: nested_tmp_name,
                                    ty: Type::Any,
                                    mutable: false,
                                    init: Some(prop_expr),
                                });
                                let nested_stmts = lower_destructuring_assignment_stmt_from_local(
                                    ctx,
                                    &ast::AssignTargetPat::Object(nested_obj.clone()),
                                    nested_tmp_id,
                                )?;
                                result.extend(nested_stmts);
                            }
                            _ => {}
                        }
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        let name = assign.key.sym.to_string();
                        let prop_expr = Expr::PropertyGet {
                            object: Box::new(Expr::LocalGet(source_id)),
                            property: name.clone(),
                        };

                        if let Some(id) = ctx.lookup_local(&name) {
                            result.push(Stmt::Expr(Expr::LocalSet(id, Box::new(prop_expr))));
                        } else {
                            return Err(anyhow!(
                                "Assignment to undeclared variable in destructuring: {}",
                                name
                            ));
                        }
                    }
                    ast::ObjectPatProp::Rest(_) => {}
                }
            }
        }
        ast::AssignTargetPat::Invalid(_) => {
            return Err(anyhow!("Invalid assignment target pattern"));
        }
    }

    Ok(result)
}
