//! `new C(args)` expression lowering: `ast::Expr::New`.
//!
//! Tier 2.3 round 3 (v0.5.339) — extracts the 393-LOC `New` arm from
//! `lower_expr`. Handles three constructor families: (a) user-defined
//! classes (lowered to `Expr::New { class_name, args }`), (b)
//! built-in JS classes routed to specialised HIR variants
//! (`new Date()` → `Expr::DateNew`, `new Map()` → `Expr::MapNew`,
//! `new RegExp()` → `Expr::RegExp`, `new Int32Array(...)` →
//! `Expr::TypedArrayNew`, etc.), (c) the dynamic
//! `new (someFn)(args)` form via `Expr::NewDynamic`.

use anyhow::{anyhow, Result};
use perry_types::LocalId;
use swc_ecma_ast as ast;

use crate::ir::Expr;
use crate::lower_decl::lower_class_from_ast;
use crate::lower_types::extract_ts_type_with_ctx;

use super::{lower_expr, LoweringContext};

pub(super) fn lower_new(ctx: &mut LoweringContext, new_expr: &ast::NewExpr) -> Result<Expr> {
    // Try to extract class name from callee
    match new_expr.callee.as_ref() {
        ast::Expr::Ident(ident) => {
            let class_name = ident.sym.to_string();

            // Handle built-in types
            if class_name == "Map" {
                // new Map() or new Map(entries)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::MapNew);
                } else {
                    return Ok(Expr::MapNewFromArray(Box::new(
                        args.into_iter().next().unwrap(),
                    )));
                }
            }
            if class_name == "Set" {
                // new Set() or new Set(iterable)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::SetNew);
                } else {
                    return Ok(Expr::SetNewFromArray(Box::new(
                        args.into_iter().next().unwrap(),
                    )));
                }
            }
            if class_name == "Date" {
                // new Date() or new Date(timestamp)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::DateNew(None));
                } else {
                    return Ok(Expr::DateNew(Some(Box::new(
                        args.into_iter().next().unwrap(),
                    ))));
                }
            }
            if class_name == "RegExp" {
                // new RegExp(pattern[, flags]) — for string-literal args,
                // route to the same `Expr::RegExp { pattern, flags }`
                // variant the literal `/foo/g` syntax produces. The
                // codegen interns both strings and calls
                // `js_regexp_new(pattern_handle, flags_handle)`.
                //
                // Without this branch, the New expression falls through
                // to generic class instantiation, which silently fails
                // (no user class named RegExp), leaving an unusable
                // ObjectHeader that makes regex.exec() return null and
                // any subsequent indexing on that null crash.
                let args_ast = new_expr.args.as_ref();
                let pattern_lit =
                    args_ast
                        .and_then(|args| args.first())
                        .and_then(|a| match a.expr.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(s)) => {
                                Some(s.value.as_str().unwrap_or("").to_string())
                            }
                            _ => None,
                        });
                let flags_lit = args_ast
                    .and_then(|args| args.get(1))
                    .and_then(|a| match a.expr.as_ref() {
                        ast::Expr::Lit(ast::Lit::Str(s)) => {
                            Some(s.value.as_str().unwrap_or("").to_string())
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                if let Some(pattern) = pattern_lit {
                    return Ok(Expr::RegExp {
                        pattern,
                        flags: flags_lit,
                    });
                }
                // Fall through to generic class instantiation for
                // non-literal args (e.g. `new RegExp(userInput)`).
                // That path is currently broken too, but at least
                // doesn't regress on the literal case which is far
                // more common.
            }
            if class_name == "Proxy" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut it = args.into_iter();
                let target = it.next().unwrap_or(Expr::Undefined);
                let handler = it.next().unwrap_or(Expr::Object(vec![]));
                return Ok(Expr::ProxyNew {
                    target: Box::new(target),
                    handler: Box::new(handler),
                });
            }
            if ctx.proxy_locals.contains(&class_name) {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                // If the proxy's construction wrapped a known class,
                // call the construct trap (for side effects) then
                // instantiate the real class. This matches the
                // test's expected behaviour.
                if let Some(target_class) = ctx.proxy_target_classes.get(&class_name).cloned() {
                    if ctx.lookup_class(&target_class).is_some() {
                        if let Some(id) = ctx.lookup_local(&class_name) {
                            let trap_call = Expr::ProxyConstruct {
                                proxy: Box::new(Expr::LocalGet(id)),
                                args: args.clone(),
                            };
                            return Ok(Expr::Sequence(vec![
                                trap_call,
                                Expr::New {
                                    class_name: target_class,
                                    args,
                                    type_args: vec![],
                                },
                            ]));
                        }
                    }
                }
                if let Some(id) = ctx.lookup_local(&class_name) {
                    return Ok(Expr::ProxyConstruct {
                        proxy: Box::new(Expr::LocalGet(id)),
                        args,
                    });
                }
            }
            // Handle AggregateError separately (2-arg form: errors array, message)
            if class_name == "AggregateError" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut iter = args.into_iter();
                let errors = iter.next().unwrap_or(Expr::Array(vec![]));
                let message = iter.next().unwrap_or(Expr::String("".to_string()));
                return Ok(Expr::AggregateErrorNew {
                    errors: Box::new(errors),
                    message: Box::new(message),
                });
            }

            // Handle Error and its subclasses
            if class_name == "Error"
                || class_name == "TypeError"
                || class_name == "RangeError"
                || class_name == "ReferenceError"
                || class_name == "SyntaxError"
                || class_name == "BugIndicatingError"
            {
                // new Error() / new Error(message) / new Error(message, { cause })
                //
                // 2-arg form detection runs at AST level (not HIR) because Phase 3
                // synthesises anon classes for closed-shape object literals — the
                // options `{ cause: e }` would become `Expr::New { __AnonShape_N }`
                // after lower_expr, and the `Expr::Object(fields)` match below
                // would miss it. Pull `cause` directly from the AST first, then
                // fall through to the standard argument lowering for other shapes.
                let ast_args = new_expr.args.as_deref().unwrap_or(&[]);
                if ast_args.len() == 2 && class_name == "Error" {
                    let msg = lower_expr(ctx, &ast_args[0].expr)?;
                    // Peel `Expr::Paren(({ cause: e }))` — SWC preserves paren
                    // nodes, so without unwrapping the outer Object match below
                    // would miss `new Error(msg, ({ cause }))` and we'd silently
                    // drop the cause.
                    let mut opts_expr: &ast::Expr = &ast_args[1].expr;
                    while let ast::Expr::Paren(p) = opts_expr {
                        opts_expr = &p.expr;
                    }
                    // Look for `{ cause: <expr> }` or `{ cause }` at the AST level.
                    if let ast::Expr::Object(opts_obj) = opts_expr {
                        for prop in &opts_obj.props {
                            if let ast::PropOrSpread::Prop(p) = prop {
                                match p.as_ref() {
                                    ast::Prop::KeyValue(kv) => {
                                        let key = match &kv.key {
                                            ast::PropName::Ident(i) => i.sym.to_string(),
                                            ast::PropName::Str(s) => {
                                                s.value.as_str().unwrap_or("").to_string()
                                            }
                                            _ => continue,
                                        };
                                        if key == "cause" {
                                            let cause = lower_expr(ctx, &kv.value)?;
                                            return Ok(Expr::ErrorNewWithCause {
                                                message: Box::new(msg),
                                                cause: Box::new(cause),
                                            });
                                        }
                                    }
                                    // ES2022 shorthand `new Error(msg, { cause })`
                                    // — the canonical idiom inside a `catch (cause)`
                                    // block. Resolve the ident the same way the
                                    // HIR Object-literal lowering does: func /
                                    // local / class-ref precedence.
                                    ast::Prop::Shorthand(ident) => {
                                        let name = ident.sym.to_string();
                                        if name != "cause" {
                                            continue;
                                        }
                                        let cause = if let Some(func_id) = ctx.lookup_func(&name) {
                                            Expr::FuncRef(func_id)
                                        } else if let Some(local_id) = ctx.lookup_local(&name) {
                                            Expr::LocalGet(local_id)
                                        } else if ctx.lookup_class(&name).is_some() {
                                            Expr::ClassRef(name.clone())
                                        } else {
                                            // Unresolvable identifier — fall through
                                            // to the no-cause path below.
                                            continue;
                                        };
                                        return Ok(Expr::ErrorNewWithCause {
                                            message: Box::new(msg),
                                            cause: Box::new(cause),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    // No recognizable `cause` key — lower the opts for side effects,
                    // then emit a plain Error with just the message.
                    let _ = lower_expr(ctx, &ast_args[1].expr)?;
                    return Ok(Expr::ErrorNew(Some(Box::new(msg))));
                }

                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();

                if args.is_empty() {
                    return match class_name.as_str() {
                        "TypeError" => {
                            Ok(Expr::TypeErrorNew(Box::new(Expr::String("".to_string()))))
                        }
                        "RangeError" => {
                            Ok(Expr::RangeErrorNew(Box::new(Expr::String("".to_string()))))
                        }
                        "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(Expr::String(
                            "".to_string(),
                        )))),
                        "SyntaxError" => {
                            Ok(Expr::SyntaxErrorNew(Box::new(Expr::String("".to_string()))))
                        }
                        _ => Ok(Expr::ErrorNew(None)),
                    };
                } else {
                    let msg = args.into_iter().next().unwrap();
                    return match class_name.as_str() {
                        "TypeError" => Ok(Expr::TypeErrorNew(Box::new(msg))),
                        "RangeError" => Ok(Expr::RangeErrorNew(Box::new(msg))),
                        "ReferenceError" => Ok(Expr::ReferenceErrorNew(Box::new(msg))),
                        "SyntaxError" => Ok(Expr::SyntaxErrorNew(Box::new(msg))),
                        _ => Ok(Expr::ErrorNew(Some(Box::new(msg)))),
                    };
                }
            }

            // Handle URL class
            if class_name == "URL" {
                // new URL(url) or new URL(url, base)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let mut args_iter = args.into_iter();
                let url_arg = args_iter
                    .next()
                    .ok_or_else(|| anyhow!("URL constructor requires at least 1 argument"))?;
                let base_arg = args_iter.next();
                return Ok(Expr::UrlNew {
                    url: Box::new(url_arg),
                    base: base_arg.map(Box::new),
                });
            }

            // Handle URLSearchParams class
            if class_name == "URLSearchParams" {
                // new URLSearchParams() or new URLSearchParams(init)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let init_arg = args.into_iter().next();
                return Ok(Expr::UrlSearchParamsNew(init_arg.map(Box::new)));
            }

            // Handle WeakRef class — wraps a value (object) in a weak reference object.
            // Pragmatic implementation: stores a strong reference and `deref()` always returns it.
            if class_name == "WeakRef" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let target = args
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow!("WeakRef constructor requires 1 argument"))?;
                return Ok(Expr::WeakRefNew(Box::new(target)));
            }

            // Handle FinalizationRegistry class — registers cleanup callbacks invoked when
            // tracked targets are GC'd. Pragmatic implementation: stores registrations but
            // never fires the callback (Perry's GC doesn't track weak references yet).
            if class_name == "FinalizationRegistry" {
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let cb = args.into_iter().next().ok_or_else(|| {
                    anyhow!("FinalizationRegistry constructor requires a callback argument")
                })?;
                return Ok(Expr::FinalizationRegistryNew(Box::new(cb)));
            }
            // Handle TextEncoder constructor
            if class_name == "TextEncoder" {
                return Ok(Expr::TextEncoderNew);
            }
            // Handle TextDecoder constructor
            if class_name == "TextDecoder" {
                // new TextDecoder() or new TextDecoder("utf-8") — we only support UTF-8
                return Ok(Expr::TextDecoderNew);
            }

            // Handle Uint8Array constructor
            if class_name == "Uint8Array" {
                // new Uint8Array() or new Uint8Array(length) or new Uint8Array(array)
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                if args.is_empty() {
                    return Ok(Expr::Uint8ArrayNew(None));
                } else if args.len() == 1 {
                    return Ok(Expr::Uint8ArrayNew(Some(Box::new(
                        args.into_iter().next().unwrap(),
                    ))));
                }
                // 2+ args: fall through to Expr::New to handle
                // new Uint8Array(buffer, byteOffset, length) etc.
            }

            // Handle other typed-array constructors (Int8/16/32, Uint16/32, Float32/64,
            // Uint8ClampedArray). Uint8Array stays on the Buffer path above.
            if let Some(kind) = crate::ir::typed_array_kind_for_name(class_name.as_str()) {
                if class_name != "Uint8Array" {
                    let args = new_expr
                        .args
                        .as_ref()
                        .map(|args| {
                            args.iter()
                                .map(|a| lower_expr(ctx, &a.expr))
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?
                        .unwrap_or_default();
                    if args.is_empty() {
                        return Ok(Expr::TypedArrayNew { kind, arg: None });
                    } else if args.len() == 1 {
                        return Ok(Expr::TypedArrayNew {
                            kind,
                            arg: Some(Box::new(args.into_iter().next().unwrap())),
                        });
                    }
                    // Multi-arg form (buffer, byteOffset, length): fall through.
                }
            }

            let mut args = new_expr
                .args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|a| lower_expr(ctx, &a.expr))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            // Extract explicit type arguments if present (e.g., new Box<number>(42))
            let type_args = new_expr
                .type_args
                .as_ref()
                .map(|ta| {
                    ta.params
                        .iter()
                        .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                        .collect()
                })
                .unwrap_or_default();
            // Issue #212: classes nested in a function may capture
            // enclosing-scope locals. `lower_class_decl` extended the
            // constructor with one synthesized param per captured id;
            // pass each as `LocalGet(id)` here so the outer scope's
            // current value is snapshotted onto the new instance.
            let class_captures: Vec<LocalId> = ctx
                .lookup_class_captures(&class_name)
                .map(|c| c.to_vec())
                .unwrap_or_default();
            for cid in class_captures {
                args.push(Expr::LocalGet(cid));
            }
            Ok(Expr::New {
                class_name,
                args,
                type_args,
            })
        }
        // Non-identifier callee (e.g., new (condition ? A : B)() or new someVar())
        _ => {
            // Check for class expressions: new (class extends X { ... })()
            let class_expr_opt = match new_expr.callee.as_ref() {
                ast::Expr::Class(ce) => Some(ce),
                ast::Expr::Paren(paren) => match paren.expr.as_ref() {
                    ast::Expr::Class(ce) => Some(ce),
                    _ => None,
                },
                _ => None,
            };
            if let Some(class_expr) = class_expr_opt {
                let synthetic_name = format!("__anon_class_{}", ctx.fresh_class());
                let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
                ctx.pending_classes.push(class);
                let args = new_expr
                    .args
                    .as_ref()
                    .map(|args| {
                        args.iter()
                            .map(|a| lower_expr(ctx, &a.expr))
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let type_args = new_expr
                    .type_args
                    .as_ref()
                    .map(|ta| {
                        ta.params
                            .iter()
                            .map(|t| extract_ts_type_with_ctx(t, Some(ctx)))
                            .collect()
                    })
                    .unwrap_or_default();
                return Ok(Expr::New {
                    class_name: synthetic_name,
                    args,
                    type_args,
                });
            }

            let callee = Box::new(lower_expr(ctx, &new_expr.callee)?);
            let args = new_expr
                .args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|a| lower_expr(ctx, &a.expr))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            Ok(Expr::NewDynamic { callee, args })
        }
    }
}
