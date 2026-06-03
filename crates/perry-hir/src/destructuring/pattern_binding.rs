//! Recursive lowering of binding patterns (`let { a, b } = expr`).

use super::*;

fn is_global_this_value(ctx: &LoweringContext, expr: &Expr) -> bool {
    matches!(expr, Expr::GlobalGet(_))
        || matches!(
            expr,
            Expr::PropertyGet { object, property }
                if matches!(object.as_ref(), Expr::GlobalGet(_))
                    && property == "globalThis"
        )
        || matches!(expr, Expr::LocalGet(id) if ctx.global_this_aliases.contains(id))
}

fn global_this_constructor_alias(name: &str) -> Option<&'static str> {
    match name {
        "URL" => Some("URL"),
        "URLSearchParams" => Some("URLSearchParams"),
        "TextEncoder" => Some("TextEncoder"),
        "TextDecoder" => Some("TextDecoder"),
        "Blob" => Some("Blob"),
        "File" => Some("File"),
        "FormData" => Some("FormData"),
        "Headers" => Some("Headers"),
        "Request" => Some("Request"),
        "Response" => Some("Response"),
        _ => None,
    }
}

fn is_global_this_fetch_constructor(name: &str) -> bool {
    matches!(
        name,
        "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
    )
}

/// Recursively lower a binding pattern against a source expression, producing
/// `Let` statements that declare each bound variable.
///
/// This is the single source of truth for destructuring binding patterns. It
/// handles:
/// - `Pat::Ident(x)`     → `let x = <source>`
/// - `Pat::Assign(p = d)`→ `let tmp = <source>; <recurse on p with tmp !== undefined ? tmp : d>`
/// - `Pat::Array([...])`→ materialize source in a temp, then recurse on each
///   element with `tmp[i]` as the source. Handles `Pat::Rest` (last element)
///   via `ArraySlice` and skips holes (`None`) like `[a, , c]`.
/// - `Pat::Object({...})`→ materialize source in a temp, then for each prop
///   recurse on the value pattern with `tmp.key` (or `tmp[expr]` for computed
///   keys) as the source. `Assign` shorthand props apply defaults inline.
///   `Rest` props use `ObjectRest` with the list of explicitly-destructured keys.
pub(crate) fn lower_pattern_binding(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    source: Expr,
    mutable: bool,
) -> Result<Vec<Stmt>> {
    let mut result = Vec::new();
    lower_pattern_binding_into(ctx, pat, source, mutable, &mut result)?;
    Ok(result)
}

pub(crate) fn lower_pattern_binding_into(
    ctx: &mut LoweringContext,
    pat: &ast::Pat,
    source: Expr,
    mutable: bool,
    result: &mut Vec<Stmt>,
) -> Result<()> {
    match pat {
        ast::Pat::Ident(ident) => {
            let name = ident.id.sym.to_string();
            let ty = ident
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Any);
            let id = ctx.define_local(name.clone(), ty.clone());
            if !mutable {
                ctx.mark_local_immutable(id);
            }
            result.push(Stmt::Let {
                id,
                name,
                ty,
                mutable,
                init: Some(source),
            });
            Ok(())
        }
        ast::Pat::Assign(assign_pat) => {
            // `p = default` — apply default when source is undefined.
            // We also need to treat bare IEEE NaN (e.g., from OOB array reads)
            // as undefined, because Perry's number arrays return NaN rather
            // than TAG_UNDEFINED for out-of-bounds indices.
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, Type::Any));
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: Type::Any,
                mutable: false,
                init: Some(source),
            });
            let default_val = lower_expr(ctx, &assign_pat.right)?;
            // If `IsUndefinedOrBareNan(tmp)` then use default, else use tmp.
            let with_default = Expr::Conditional {
                condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(Expr::LocalGet(tmp_id)))),
                then_expr: Box::new(default_val),
                else_expr: Box::new(Expr::LocalGet(tmp_id)),
            };
            lower_pattern_binding_into(ctx, &assign_pat.left, with_default, mutable, result)
        }
        ast::Pat::Array(arr_pat) => {
            // Materialize source into a temp
            let arr_ty = arr_pat
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Array(Box::new(Type::Any)));
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, arr_ty.clone()));
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: arr_ty,
                mutable: false,
                init: Some(source),
            });

            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                let Some(elem_pat) = elem else { continue }; // hole — skip

                if let ast::Pat::Rest(rest_pat) = elem_pat {
                    // Rest element `...rest` — take remaining elements as an array
                    let slice_expr = Expr::ArraySlice {
                        array: Box::new(Expr::LocalGet(tmp_id)),
                        start: Box::new(Expr::Number(idx as f64)),
                        end: None,
                    };
                    lower_pattern_binding_into(ctx, &rest_pat.arg, slice_expr, mutable, result)?;
                    break; // Rest must be last
                }

                let element_source = Expr::IndexGet {
                    object: Box::new(Expr::LocalGet(tmp_id)),
                    index: Box::new(Expr::Number(idx as f64)),
                };
                lower_pattern_binding_into(ctx, elem_pat, element_source, mutable, result)?;
            }
            Ok(())
        }
        ast::Pat::Object(obj_pat) => {
            // Materialize source into a temp
            let source_is_global_this = is_global_this_value(ctx, &source);
            let obj_ty = obj_pat
                .type_ann
                .as_ref()
                .map(|ann| extract_ts_type(&ann.type_ann))
                .unwrap_or(Type::Any);
            let tmp_id = ctx.fresh_local();
            let tmp_name = format!("__destruct_{}", tmp_id);
            ctx.locals.push((tmp_name.clone(), tmp_id, obj_ty.clone()));
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: obj_ty,
                mutable: false,
                init: Some(source),
            });

            // Collect statically-known keys for rest exclusion tracking.
            let mut static_keys: Vec<String> = Vec::new();

            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        let key_source = match &kv.key {
                            ast::PropName::Ident(ident) => {
                                let key = ident.sym.to_string();
                                static_keys.push(key.clone());
                                if source_is_global_this {
                                    if let ast::Pat::Ident(alias) = kv.value.as_ref() {
                                        if let Some(class_name) =
                                            global_this_constructor_alias(&key)
                                        {
                                            ctx.register_let_class_alias(
                                                alias.id.sym.to_string(),
                                                class_name.to_string(),
                                            );
                                            if is_global_this_fetch_constructor(class_name) {
                                                ctx.uses_fetch = true;
                                            }
                                        }
                                    }
                                }
                                Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: key,
                                }
                            }
                            ast::PropName::Str(s) => {
                                let key = s.value.as_str().unwrap_or("").to_string();
                                static_keys.push(key.clone());
                                if source_is_global_this {
                                    if let ast::Pat::Ident(alias) = kv.value.as_ref() {
                                        if let Some(class_name) =
                                            global_this_constructor_alias(&key)
                                        {
                                            ctx.register_let_class_alias(
                                                alias.id.sym.to_string(),
                                                class_name.to_string(),
                                            );
                                            if is_global_this_fetch_constructor(class_name) {
                                                ctx.uses_fetch = true;
                                            }
                                        }
                                    }
                                }
                                Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: key,
                                }
                            }
                            ast::PropName::Num(n) => {
                                let key = n.value.to_string();
                                static_keys.push(key.clone());
                                Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: key,
                                }
                            }
                            ast::PropName::Computed(computed) => {
                                // Computed key: const { [prop]: target } = obj
                                // Lower to IndexGet with the computed expression
                                let index_expr = lower_expr(ctx, &computed.expr)?;
                                Expr::IndexGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    index: Box::new(index_expr),
                                }
                            }
                            ast::PropName::BigInt(_) => continue,
                        };
                        lower_pattern_binding_into(ctx, &kv.value, key_source, mutable, result)?;
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        // Shorthand { key } or { key = default }
                        let name = assign.key.sym.to_string();
                        static_keys.push(name.clone());
                        if source_is_global_this {
                            if let Some(class_name) = global_this_constructor_alias(&name) {
                                ctx.register_let_class_alias(name.clone(), class_name.to_string());
                                if is_global_this_fetch_constructor(class_name) {
                                    ctx.uses_fetch = true;
                                }
                            }
                        }
                        let ty = assign
                            .key
                            .type_ann
                            .as_ref()
                            .map(|ann| extract_ts_type(&ann.type_ann))
                            .unwrap_or(Type::Any);
                        let id = ctx.define_local(name.clone(), ty.clone());

                        let init_value = if let Some(default_expr) = &assign.value {
                            // Materialize the property read into a temp so we
                            // only evaluate it once (important if the property
                            // getter is side-effecting, but also required for
                            // correct NaN detection).
                            let val_tmp_id = ctx.fresh_local();
                            let val_tmp_name = format!("__destruct_{}", val_tmp_id);
                            ctx.locals
                                .push((val_tmp_name.clone(), val_tmp_id, Type::Any));
                            result.push(Stmt::Let {
                                id: val_tmp_id,
                                name: val_tmp_name,
                                ty: Type::Any,
                                mutable: false,
                                init: Some(Expr::PropertyGet {
                                    object: Box::new(Expr::LocalGet(tmp_id)),
                                    property: name.clone(),
                                }),
                            });
                            let default_val = lower_expr(ctx, default_expr)?;
                            Expr::Conditional {
                                condition: Box::new(Expr::IsUndefinedOrBareNan(Box::new(
                                    Expr::LocalGet(val_tmp_id),
                                ))),
                                then_expr: Box::new(default_val),
                                else_expr: Box::new(Expr::LocalGet(val_tmp_id)),
                            }
                        } else {
                            Expr::PropertyGet {
                                object: Box::new(Expr::LocalGet(tmp_id)),
                                property: name.clone(),
                            }
                        };
                        result.push(Stmt::Let {
                            id,
                            name,
                            ty,
                            mutable,
                            init: Some(init_value),
                        });
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        // { ...rest } — collect remaining statically-known keys
                        // and use ObjectRest to clone the object without them.
                        let rest_source = Expr::ObjectRest {
                            object: Box::new(Expr::LocalGet(tmp_id)),
                            exclude_keys: static_keys.clone(),
                        };
                        lower_pattern_binding_into(ctx, &rest.arg, rest_source, mutable, result)?;
                        break; // Rest must be last
                    }
                }
            }
            Ok(())
        }
        ast::Pat::Rest(_) => {
            // Rest patterns should be handled by their enclosing Array/Object
            Err(anyhow!(
                "Rest pattern outside of array/object destructuring"
            ))
        }
        ast::Pat::Expr(_) => Err(anyhow!(
            "Expression patterns are not supported in binding destructuring"
        )),
        ast::Pat::Invalid(_) => Err(anyhow!("Invalid binding pattern")),
    }
}
