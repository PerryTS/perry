//! Lowering of destructuring assignment expressions (`[a, b] = expr` as a value).

use super::*;

/// Lower a destructuring assignment expression.
/// For [a, b] = expr or { a, b } = expr, we generate a Sequence expression:
///   1. Assign each element/property to the corresponding target
///   2. Return the RHS value (assignment expressions evaluate to RHS)
///
/// Note: We reference the RHS value directly multiple times rather than
/// creating a temporary variable, since temps created in expression context
/// aren't visible to codegen. This is safe when the RHS is a simple expression
/// (which is the common case for destructuring).
pub(crate) fn lower_destructuring_assignment(
    ctx: &mut LoweringContext,
    pat: &ast::AssignTargetPat,
    value: Box<Expr>,
) -> Result<Expr> {
    match pat {
        ast::AssignTargetPat::Array(arr_pat) => {
            // Array destructuring assignment: [a, b] = expr
            // Desugar to:
            //   a = expr[0];
            //   b = expr[1];
            //   expr (result)
            //
            // We reference the RHS value directly. This works because:
            // 1. The RHS is typically a local variable or simple expression
            // 2. Creating a temp in expression context is problematic for codegen

            let mut exprs = Vec::new();

            // Now assign each element
            for (idx, elem) in arr_pat.elems.iter().enumerate() {
                if let Some(elem_pat) = elem {
                    let index_expr = Expr::IndexGet {
                        object: value.clone(),
                        index: Box::new(Expr::Number(idx as f64)),
                    };

                    match elem_pat {
                        ast::Pat::Ident(ident) => {
                            let name = ident.id.sym.to_string();
                            if let Some(id) = ctx.lookup_local(&name) {
                                exprs.push(Expr::LocalSet(id, Box::new(index_expr)));
                            } else {
                                return Err(anyhow!(
                                    "Assignment to undeclared variable in destructuring: {}",
                                    name
                                ));
                            }
                        }
                        ast::Pat::Expr(inner_expr) => {
                            // Expression pattern like [obj.prop] = arr
                            match inner_expr.as_ref() {
                                ast::Expr::Member(member) => {
                                    let object = Box::new(lower_expr(ctx, &member.obj)?);
                                    match &member.prop {
                                        ast::MemberProp::Ident(prop_ident) => {
                                            let property = prop_ident.sym.to_string();
                                            exprs.push(Expr::PropertySet {
                                                object,
                                                property,
                                                value: Box::new(index_expr),
                                            });
                                        }
                                        ast::MemberProp::Computed(computed) => {
                                            let index = Box::new(lower_expr(ctx, &computed.expr)?);
                                            exprs.push(Expr::IndexSet {
                                                object,
                                                index,
                                                value: Box::new(index_expr),
                                            });
                                        }
                                        // `[this.#field] = arr` — brand-guard the
                                        // receiver so a wrong-receiver write throws.
                                        ast::MemberProp::PrivateName(private) => {
                                            let property = format!("#{}", private.name);
                                            let object = crate::lower::wrap_private_guard(
                                                ctx,
                                                object,
                                                &property,
                                                crate::lower::PRIV_OP_WRITE,
                                            );
                                            exprs.push(Expr::PropertySet {
                                                object,
                                                property,
                                                value: Box::new(index_expr),
                                            });
                                        }
                                        _ => {
                                            return Err(anyhow!(
                                                "Unsupported member expression in destructuring"
                                            ));
                                        }
                                    }
                                }
                                _ => {
                                    return Err(anyhow!(
                                        "Unsupported expression pattern in destructuring"
                                    ));
                                }
                            }
                        }
                        ast::Pat::Rest(_) => {
                            // Rest pattern in assignment: [...rest] = arr
                            // For now, skip (would need slice operation)
                        }
                        ast::Pat::Array(nested_arr) => {
                            // Nested array destructuring: [[a, b], c] = expr
                            // Recursively lower with the indexed element as the value
                            let nested_target = ast::AssignTargetPat::Array(nested_arr.clone());
                            let nested_expr = lower_destructuring_assignment(
                                ctx,
                                &nested_target,
                                Box::new(index_expr),
                            )?;
                            exprs.push(nested_expr);
                        }
                        ast::Pat::Object(nested_obj) => {
                            // Nested object destructuring: [{ a, b }, c] = expr
                            let nested_target = ast::AssignTargetPat::Object(nested_obj.clone());
                            let nested_expr = lower_destructuring_assignment(
                                ctx,
                                &nested_target,
                                Box::new(index_expr),
                            )?;
                            exprs.push(nested_expr);
                        }
                        _ => {
                            // Other patterns (Assign with default, etc.) - skip for now
                        }
                    }
                }
                // If elem is None, it's a hole like [a, , c] - skip it
            }

            // The result of the assignment is the original RHS value
            exprs.push(*value);

            Ok(Expr::Sequence(exprs))
        }
        ast::AssignTargetPat::Object(obj_pat) => {
            // Object destructuring assignment: { a, b } = expr
            // Desugar to:
            //   a = expr.a;
            //   b = expr.b;
            //   expr (result)

            let mut exprs = Vec::new();
            // Computed keys destructured here (`{ [k]: t }`), lowered once for the
            // rest exclusion below. This is the value-used assignment-expression
            // form (`y = ({ [k]: t, ...rest } = o)`); it builds a pure-expression
            // Sequence with no statement slots, so a computed key can't be spilled
            // to a shared temp and is re-lowered for the exclusion — a
            // side-effecting computed key evaluates twice in this rare form.
            let mut computed_excl_keys: Vec<Expr> = Vec::new();

            // Now assign each property
            for prop in &obj_pat.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => {
                        // { key: target } - extract obj.key into target
                        let prop_expr = match &kv.key {
                            ast::PropName::Ident(ident) => Expr::PropertyGet {
                                object: value.clone(),
                                property: ident.sym.to_string(),
                            },
                            ast::PropName::Str(s) => Expr::PropertyGet {
                                object: value.clone(),
                                property: s.value.as_str().unwrap_or("").to_string(),
                            },
                            ast::PropName::Num(n) => Expr::PropertyGet {
                                object: value.clone(),
                                property: n.value.to_string(),
                            },
                            ast::PropName::Computed(computed) => {
                                // `{ [k]: target }` → target = obj[k]; also record
                                // the key so `...rest` can exclude it below.
                                computed_excl_keys.push(lower_expr(ctx, &computed.expr)?);
                                Expr::IndexGet {
                                    object: value.clone(),
                                    index: Box::new(lower_expr(ctx, &computed.expr)?),
                                }
                            }
                            _ => continue, // BigInt property names, etc.
                        };

                        match &*kv.value {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    exprs.push(Expr::LocalSet(id, Box::new(prop_expr)));
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring: {}",
                                        name
                                    ));
                                }
                            }
                            ast::Pat::Array(nested_arr) => {
                                let nested_target = ast::AssignTargetPat::Array(nested_arr.clone());
                                let nested_expr = lower_destructuring_assignment(
                                    ctx,
                                    &nested_target,
                                    Box::new(prop_expr),
                                )?;
                                exprs.push(nested_expr);
                            }
                            ast::Pat::Object(nested_obj) => {
                                let nested_target =
                                    ast::AssignTargetPat::Object(nested_obj.clone());
                                let nested_expr = lower_destructuring_assignment(
                                    ctx,
                                    &nested_target,
                                    Box::new(prop_expr),
                                )?;
                                exprs.push(nested_expr);
                            }
                            _ => {
                                // Other patterns - skip for now
                            }
                        }
                    }
                    ast::ObjectPatProp::Assign(assign) => {
                        // Shorthand: { a } means { a: a }
                        let name = assign.key.sym.to_string();
                        let prop_expr = Expr::PropertyGet {
                            object: value.clone(),
                            property: name.clone(),
                        };

                        if let Some(id) = ctx.lookup_local(&name) {
                            exprs.push(Expr::LocalSet(id, Box::new(prop_expr)));
                        } else {
                            return Err(anyhow!(
                                "Assignment to undeclared variable in destructuring: {}",
                                name
                            ));
                        }
                    }
                    ast::ObjectPatProp::Rest(rest) => {
                        // `({ a, b, ...rest } = obj)` → rest = ObjectRest(obj, ["a","b"]).
                        // Previously skipped, leaving `rest` undefined — which broke
                        // every React-Compiler-memoized component that spreads its
                        // remaining props via `...rest` (e.g. ink's Box/Text pass
                        // the style props through the rest), collapsing layout to
                        // defaults. Collect the statically-named keys to exclude and
                        // bind the rest to a fresh object of the remaining own keys.
                        let exclude_keys =
                            super::helpers::collect_static_object_pattern_keys(&obj_pat.props);
                        let rest_expr = Expr::ObjectRest {
                            object: value.clone(),
                            exclude_keys,
                        };
                        match &*rest.arg {
                            ast::Pat::Ident(ident) => {
                                let name = ident.id.sym.to_string();
                                if let Some(id) = ctx.lookup_local(&name) {
                                    exprs.push(Expr::LocalSet(id, Box::new(rest_expr)));
                                    // ObjectRest only excludes statically-named
                                    // keys; delete the computed ones from the fresh
                                    // rest object (a private copy, so the source is
                                    // untouched).
                                    for key in computed_excl_keys.drain(..) {
                                        exprs.push(Expr::Delete(Box::new(Expr::IndexGet {
                                            object: Box::new(Expr::LocalGet(id)),
                                            index: Box::new(key),
                                        })));
                                    }
                                } else {
                                    return Err(anyhow!(
                                        "Assignment to undeclared variable in destructuring rest: {}",
                                        name
                                    ));
                                }
                            }
                            // A non-identifier object-rest target (e.g. `({...a.b} = q)`)
                            // is legal JS but not emitted by real codegen (the React
                            // Compiler always binds a fresh identifier); fail loudly
                            // instead of silently dropping the rest.
                            _ => {
                                return Err(anyhow!(
                                    "Unsupported object-rest assignment target: only a plain identifier is supported"
                                ));
                            }
                        }
                    }
                }
            }

            // The result of the assignment is the original RHS value
            exprs.push(*value);

            Ok(Expr::Sequence(exprs))
        }
        ast::AssignTargetPat::Invalid(_) => Err(anyhow!("Invalid assignment target pattern")),
    }
}
