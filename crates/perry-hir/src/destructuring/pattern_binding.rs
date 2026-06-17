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

/// Allocate a fresh internal local for destructuring scaffolding.
fn fresh_destruct_local(ctx: &mut LoweringContext, ty: Type) -> (LocalId, String) {
    let id = ctx.fresh_local();
    let name = format!("__destruct_{}", id);
    ctx.locals.push((name.clone(), id, ty));
    (id, name)
}

/// Lower a destructuring-default initializer, applying NamedEvaluation when the
/// binding target is a single name and the initializer is an anonymous function
/// / arrow / class. Per spec (SingleNameBinding / KeyedBindingInitialization),
/// `let { x = function(){} } = {}` and `let [ x = () => {} ] = []` name the
/// function after `x` (`x.name === "x"`). The closure-lowering paths read
/// `ctx.assignment_inferred_name`; a named function expression ignores it, so
/// `[ xFn = function x(){} ]` correctly keeps `"x"`.
fn lower_default_named(
    ctx: &mut LoweringContext,
    default_expr: &ast::Expr,
    binding_name: Option<&str>,
) -> Result<Expr> {
    if let Some(name) = binding_name {
        if crate::lower::expr_assign::rhs_accepts_assignment_name(default_expr) {
            let old = ctx.assignment_inferred_name.replace(name.to_string());
            let result = lower_expr(ctx, default_expr);
            ctx.assignment_inferred_name = old;
            return result;
        }
    }
    lower_expr(ctx, default_expr)
}

/// The binding name to use for NamedEvaluation of a `Pat::Assign` target — only
/// a single `BindingIdentifier` qualifies (nested array/object patterns don't).
fn single_name_target(pat: &ast::Pat) -> Option<String> {
    match pat {
        ast::Pat::Ident(ident) => Some(ident.id.sym.to_string()),
        _ => None,
    }
}

/// Build a call into the runtime iterator-protocol helpers (same dispatch the
/// assignment-destructuring path uses).
fn runtime_iterator_call(method: &str, args: Vec<Expr>) -> Expr {
    Expr::NativeMethodCall {
        module: "__perry_runtime".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }
}

/// `value === undefined` — the spec check that gates a destructuring default
/// (`[a = d]`, `{a = d}`). Distinct from `IsUndefinedOrBareNan`: a genuine NaN
/// element/property must NOT trigger the default (`let [a = 1] = [NaN]` → NaN).
fn is_strictly_undefined(value: Expr) -> Expr {
    Expr::Compare {
        op: CompareOp::Eq,
        left: Box::new(value),
        right: Box::new(Expr::Undefined),
    }
}

/// Pull the next value from `iter` into `value_id`, honoring the `done` flag.
/// Mirrors the assignment-destructuring `iterator_next_value_stmts`:
///   if (done) { value = undefined; }
///   else { step = IteratorNext(iter); if (step.done) { done = true; value = undefined } else value = step.value }
fn iterator_next_value_stmts(
    ctx: &mut LoweringContext,
    iter_id: LocalId,
    done_id: LocalId,
    value_id: LocalId,
) -> Vec<Stmt> {
    let (step_id, step_name) = fresh_destruct_local(ctx, Type::Any);
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

/// Lower an array binding pattern using the iterator protocol (spec
/// §8.5.2/8.5.3 IteratorBindingInitialization). Replaces the legacy index-based
/// (`tmp[0]`, `tmp[1]`) lowering, which never invoked `Symbol.iterator`, never
/// closed the iterator, and could not destructure non-array iterables. Each
/// element pulls via `IteratorStep`/`IteratorValue`; holes still advance the
/// iterator; a rest element drains the remainder; and the iterator is closed
/// (`IteratorClose`) on both normal completion (when not exhausted) and on any
/// abrupt completion from a default initializer or nested pattern.
fn lower_array_pattern_binding(
    ctx: &mut LoweringContext,
    arr_pat: &ast::ArrayPat,
    source: Expr,
    mutable: bool,
    result: &mut Vec<Stmt>,
) -> Result<()> {
    let (iter_id, iter_name) = fresh_destruct_local(ctx, Type::Any);
    result.push(Stmt::Let {
        id: iter_id,
        name: iter_name,
        ty: Type::Any,
        mutable: false,
        init: Some(Expr::GetIterator(Box::new(source))),
    });
    let (done_id, done_name) = fresh_destruct_local(ctx, Type::Boolean);
    result.push(Stmt::Let {
        id: done_id,
        name: done_name,
        ty: Type::Boolean,
        mutable: true,
        init: Some(Expr::Bool(false)),
    });

    let mut body: Vec<Stmt> = Vec::new();
    for elem in &arr_pat.elems {
        match elem {
            // Elision (`[, x]`) — advance the iterator and discard the value.
            None => {
                let (value_id, value_name) = fresh_destruct_local(ctx, Type::Any);
                body.push(Stmt::Let {
                    id: value_id,
                    name: value_name,
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Undefined),
                });
                body.extend(iterator_next_value_stmts(ctx, iter_id, done_id, value_id));
            }
            // Rest element (`[...rest]`) — drain the remainder into an array.
            Some(ast::Pat::Rest(rest_pat)) => {
                let (rest_id, rest_name) = fresh_destruct_local(ctx, Type::Any);
                body.push(Stmt::Let {
                    id: rest_id,
                    name: rest_name,
                    ty: Type::Any,
                    mutable: false,
                    init: Some(runtime_iterator_call(
                        "iteratorRestToArray",
                        vec![Expr::LocalGet(iter_id), Expr::LocalGet(done_id)],
                    )),
                });
                // Draining exhausts the iterator, so it is now done.
                body.push(Stmt::Expr(Expr::LocalSet(
                    done_id,
                    Box::new(Expr::Bool(true)),
                )));
                lower_pattern_binding_into(
                    ctx,
                    &rest_pat.arg,
                    Expr::LocalGet(rest_id),
                    mutable,
                    &mut body,
                )?;
                break;
            }
            Some(elem_pat) => {
                let (value_id, value_name) = fresh_destruct_local(ctx, Type::Any);
                body.push(Stmt::Let {
                    id: value_id,
                    name: value_name,
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Undefined),
                });
                body.extend(iterator_next_value_stmts(ctx, iter_id, done_id, value_id));

                // A `Pat::Assign` element carries a default initializer that is
                // evaluated lazily, only when the pulled value is `undefined`.
                if let ast::Pat::Assign(assign_pat) = elem_pat {
                    let default_val = lower_default_named(
                        ctx,
                        &assign_pat.right,
                        single_name_target(&assign_pat.left).as_deref(),
                    )?;
                    let with_default = Expr::Conditional {
                        condition: Box::new(is_strictly_undefined(Expr::LocalGet(value_id))),
                        then_expr: Box::new(default_val),
                        else_expr: Box::new(Expr::LocalGet(value_id)),
                    };
                    lower_pattern_binding_into(
                        ctx,
                        &assign_pat.left,
                        with_default,
                        mutable,
                        &mut body,
                    )?;
                } else {
                    lower_pattern_binding_into(
                        ctx,
                        elem_pat,
                        Expr::LocalGet(value_id),
                        mutable,
                        &mut body,
                    )?;
                }
            }
        }
    }

    // Close the iterator: on any abrupt completion from the body (default
    // initializer / nested pattern throwing), and again on normal completion
    // when the iterator was not exhausted.
    let close_stmt = Stmt::Expr(runtime_iterator_call(
        "iteratorCloseIfNotDone",
        vec![Expr::LocalGet(iter_id), Expr::LocalGet(done_id)],
    ));
    let (exc_id, exc_name) = fresh_destruct_local(ctx, Type::Any);
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

    Ok(())
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
            // Reuse a forward-pre-registered (boxed) local when an earlier
            // closure captured this destructured binding before its declaration
            // (the function-body Phase 1.6 pass, span-keyed). Without this the
            // closure's box and this binding's slot diverge.
            let id = match ctx.lexical_forward_decls.remove(&ident.id.span.lo.0) {
                Some(pre_id) => {
                    if let Some((_, _, ety)) = ctx
                        .locals
                        .iter_mut()
                        .rev()
                        .find(|(_, lid, _)| *lid == pre_id)
                    {
                        *ety = ty.clone();
                    }
                    pre_id
                }
                None => ctx.define_local(name.clone(), ty.clone()),
            };
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
            let default_val = lower_default_named(
                ctx,
                &assign_pat.right,
                single_name_target(&assign_pat.left).as_deref(),
            )?;
            // A destructuring default applies only when the source is strictly
            // `undefined` (not a genuine NaN value — `let [a = 1] = [NaN]` → NaN).
            let with_default = Expr::Conditional {
                condition: Box::new(is_strictly_undefined(Expr::LocalGet(tmp_id))),
                then_expr: Box::new(default_val),
                else_expr: Box::new(Expr::LocalGet(tmp_id)),
            };
            lower_pattern_binding_into(ctx, &assign_pat.left, with_default, mutable, result)
        }
        ast::Pat::Array(arr_pat) => {
            // Array binding patterns use the iterator protocol (GetIterator /
            // IteratorStep / IteratorValue / IteratorClose), per spec — not raw
            // index reads. See `lower_array_pattern_binding`.
            lower_array_pattern_binding(ctx, arr_pat, source, mutable, result)
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
            // RequireObjectCoercible: destructuring a `null`/`undefined` source
            // throws a TypeError even for an empty pattern `{}`, before any
            // property is read.
            //
            // #5247 (coverage gap): carry the object-pattern's source byte
            // offset (`obj_pat.span.lo.0`) as a second argument so codegen can,
            // under `--debug-symbols`, attach a `file:line` to the
            // "Cannot convert undefined or null to object" throw — otherwise the
            // last-set call location (often in an unrelated module) is rendered.
            // The offset is a plain `f64` literal that the default-build codegen
            // arm ignores (it reads `args.first()` only), so emitted output is
            // unchanged when the flag is off.
            result.push(Stmt::Let {
                id: tmp_id,
                name: tmp_name,
                ty: obj_ty,
                mutable: false,
                init: Some(runtime_iterator_call(
                    "requireObjectCoercible",
                    vec![source, Expr::Number(f64::from(obj_pat.span.lo.0))],
                )),
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
                        // Reuse a forward-pre-registered (boxed) local when an
                        // earlier closure forward-captured this `{ key }`
                        // shorthand binding (Phase 1.6, span-keyed) — e.g. the
                        // Next.js tracer's `_export(exports, { SpanKind: () =>
                        // SpanKind })` getter referencing the later `const {
                        // SpanKind } = api`.
                        let id = match ctx.lexical_forward_decls.remove(&assign.key.span.lo.0) {
                            Some(pre_id) => {
                                if let Some((_, _, ety)) = ctx
                                    .locals
                                    .iter_mut()
                                    .rev()
                                    .find(|(_, lid, _)| *lid == pre_id)
                                {
                                    *ety = ty.clone();
                                }
                                pre_id
                            }
                            None => ctx.define_local(name.clone(), ty.clone()),
                        };

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
                            let default_val =
                                lower_default_named(ctx, default_expr, Some(name.as_str()))?;
                            Expr::Conditional {
                                condition: Box::new(is_strictly_undefined(Expr::LocalGet(
                                    val_tmp_id,
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
