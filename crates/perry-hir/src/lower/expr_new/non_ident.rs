//! Non-identifier `new` callee lowering plus the ReadableStream/TransformStream
//! controller-param registration hook, extracted from `expr_new.rs`. Pure code
//! move — no behavior change.

use super::*;

use anyhow::Result;
use perry_types::LocalId;
use swc_ecma_ast as ast;

use crate::ir::Expr;
use crate::lower_decl::lower_class_from_ast;
use crate::lower_types::extract_ts_type_with_ctx;

use super::super::expr_new_builtins::{global_member_constructor_name, module_constructor_name};
use super::super::{lower_expr, LoweringContext};

/// Issue #237: pre-register the controller param of every
/// `start` / `pull` / `cancel` / `transform` / `flush` callback
/// passed to `new ReadableStream({...})` / `new TransformStream({...})` as a
/// native instance so `controller.enqueue(...)` etc. dispatch through the
/// streams arms in lower_call.rs. Side-effect only.
pub(crate) fn register_stream_controller_params(
    ctx: &mut LoweringContext,
    new_expr: &ast::NewExpr,
) {
    if let ast::Expr::Ident(ident) = new_expr.callee.as_ref() {
        let cls = ident.sym.as_ref();
        let field_specs: &[(&'static str, usize, &'static str, &'static str)] = match cls {
            "ReadableStream" => &[
                ("start", 0, "readable_stream", "ReadableStream"),
                ("pull", 0, "readable_stream", "ReadableStream"),
            ],
            "TransformStream" => &[
                ("transform", 1, "readable_stream", "ReadableStream"),
                ("flush", 0, "readable_stream", "ReadableStream"),
            ],
            _ => &[],
        };
        if !field_specs.is_empty() {
            if let Some(args) = new_expr.args.as_ref() {
                if let Some(first) = args.first() {
                    if let ast::Expr::Object(obj_lit) = first.expr.as_ref() {
                        for prop in &obj_lit.props {
                            if let ast::PropOrSpread::Prop(boxed_prop) = prop {
                                let mut handled = false;
                                match boxed_prop.as_ref() {
                                    ast::Prop::KeyValue(kv) => {
                                        let n = match &kv.key {
                                            ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                                            ast::PropName::Str(s) => s.value.as_str(),
                                            _ => None,
                                        };
                                        if let Some(name) = n {
                                            if let Some((_, idx, mod_name, class_name)) =
                                                field_specs.iter().find(|(f, _, _, _)| *f == name)
                                            {
                                                let pat: Option<&ast::Pat> = match kv.value.as_ref()
                                                {
                                                    ast::Expr::Arrow(arrow) => {
                                                        arrow.params.get(*idx)
                                                    }
                                                    ast::Expr::Fn(fn_expr) => fn_expr
                                                        .function
                                                        .params
                                                        .get(*idx)
                                                        .map(|p| &p.pat),
                                                    _ => None,
                                                };
                                                if let Some(ast::Pat::Ident(pid)) = pat {
                                                    ctx.register_native_instance(
                                                        pid.id.sym.to_string(),
                                                        mod_name.to_string(),
                                                        class_name.to_string(),
                                                    );
                                                    handled = true;
                                                }
                                            }
                                        }
                                    }
                                    ast::Prop::Method(m) => {
                                        let n = match &m.key {
                                            ast::PropName::Ident(i) => Some(i.sym.as_ref()),
                                            ast::PropName::Str(s) => s.value.as_str(),
                                            _ => None,
                                        };
                                        if let Some(name) = n {
                                            if let Some((_, idx, mod_name, class_name)) =
                                                field_specs.iter().find(|(f, _, _, _)| *f == name)
                                            {
                                                if let Some(param) = m.function.params.get(*idx) {
                                                    if let ast::Pat::Ident(pid) = &param.pat {
                                                        ctx.register_native_instance(
                                                            pid.id.sym.to_string(),
                                                            mod_name.to_string(),
                                                            class_name.to_string(),
                                                        );
                                                        handled = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                let _ = handled;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Non-identifier callee (e.g. `new (condition ? A : B)()` or `new someVar()`),
/// including the `new (class extends X { ... })()` class-expression form.
pub(crate) fn lower_new_non_ident(
    ctx: &mut LoweringContext,
    new_expr: &ast::NewExpr,
    callee_expr: &ast::Expr,
    new_byte_offset: u32,
) -> Result<Expr> {
    // Check for class expressions: new (class extends X { ... })()
    let class_expr_opt = match callee_expr {
        ast::Expr::Class(ce) => Some(ce),
        ast::Expr::Paren(paren) => match paren.expr.as_ref() {
            ast::Expr::Class(ce) => Some(ce),
            _ => None,
        },
        _ => None,
    };
    if let Some(class_expr) = class_expr_opt {
        let synthetic_name = format!("__anon_class_{}", ctx.fresh_class());
        ctx.pending_class_inner_name = class_expr.ident.as_ref().map(|i| i.sym.to_string());
        let class = lower_class_from_ast(ctx, &class_expr.class, &synthetic_name, false)?;
        // #6336: a class expression's `Subclass → Parent` registry edge is a SIDE
        // EFFECT of evaluating the expression — `lower_class_expr` sequences a
        // `RegisterClassParentDynamic` in front of the `ClassRef` it yields
        // (`const K = class extends Event {}` works because of it). This arm
        // never went through `lower_class_expr`: it lowers the class straight to
        // a `New` on the synthetic name, so for a class expression constructed
        // IN PLACE the registration never ran and the instance came out
        // parentless — `new (class extends Event {})("tick") instanceof Event`
        // was `false`, and every chain walk that identifies a receiver by its
        // base (`instanceof`, the native-base init, Event/EventTarget dispatch)
        // bailed. Sequence the same registration in front of the `New` so the
        // edge is wired before the constructor runs.
        //
        // Only heritage that resolves to a runtime VALUE carries `extends_expr`
        // (a builtin like `Event`, a factory call, a captured local). A parent
        // that is a known user class in this module carries a static `extends`
        // link instead and needs no registration — which is why the user-parent
        // form of this shape already worked.
        let parent_expr = class.extends_expr.clone();
        ctx.pending_classes.push(class);
        let mut args: Vec<Expr> = new_expr
            .args
            .as_ref()
            .map(|args| {
                args.iter()
                    .map(|a| lower_expr(ctx, &a.expr))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        // Issue #212 (anon-class-expression parity): a class expression
        // nested in a function may capture enclosing-scope locals.
        // `lower_class_from_ast` → `synthesize_class_captures` extended
        // the synthesized constructor with one param per captured id and
        // rewrote the METHOD bodies to read `this.__perry_cap_<id>`. The
        // named-class `new C()` path above forwards those captures as
        // `LocalGet(id)`; the directly-constructed anonymous form
        // (`new class { m() { return outer } }()`) must do the same, or
        // the cap params receive `undefined` and every method that reads
        // a captured local sees `undefined`. Refs Next.js bundled tracer
        // (`getActiveScopeSpan` → `trace.getSpan` on undefined `trace`).
        let class_captures: Vec<LocalId> = ctx
            .lookup_class_captures(&synthetic_name)
            .map(|c| c.to_vec())
            .unwrap_or_default();
        // #6538: record the appended cap-forward count explicitly (see the
        // named-class arm in `expr_new.rs` and the `Expr::New` docs).
        let cap_args_appended = class_captures.len() as u32;
        for cid in class_captures {
            args.push(Expr::LocalGet(cid));
        }
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
        let construct = Expr::New {
            class_name: synthetic_name.clone(),
            args,
            type_args,
            byte_offset: new_byte_offset,
            cap_args_appended,
        };
        // The `Sequence` yields its LAST element, so the `new` site still sees
        // the constructed instance — the registration is pure side effect,
        // ordered before it.
        let Some(parent_expr) = parent_expr else {
            return Ok(construct);
        };
        return Ok(Expr::Sequence(vec![
            Expr::RegisterClassParentDynamic {
                class_name: synthetic_name,
                parent_expr,
            },
            construct,
        ]));
    }

    let callee = Box::new(lower_expr(ctx, callee_expr)?);
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
    if let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    {
        if is_global_object_expr(ctx, object.as_ref())
            && matches!(property.as_str(), "Symbol" | "BigInt" | "Math")
        {
            return Ok(nonconstructable_builtin_throw_expr(property, args));
        }
        if is_global_object_expr(ctx, object.as_ref())
            && matches!(
                property.as_str(),
                "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response" | "WebSocket"
            )
        {
            if is_fetch_constructor_name(property) {
                ctx.uses_fetch = true;
            }
            return Ok(Expr::New {
                class_name: property.clone(),
                args,
                type_args: Vec::new(),
                byte_offset: new_byte_offset,
                cap_args_appended: 0,
            });
        }
        if matches!(object.as_ref(), Expr::NativeModuleRef(module)
            if module == "buffer" || module == "node:buffer")
            && matches!(property.as_str(), "Blob" | "File")
        {
            ctx.uses_fetch = true;
            return Ok(Expr::New {
                class_name: property.clone(),
                args,
                type_args: Vec::new(),
                byte_offset: new_byte_offset,
                cap_args_appended: 0,
            });
        }
    }
    Ok(Expr::NewDynamic {
        callee,
        args,
        byte_offset: new_byte_offset,
    })
}
