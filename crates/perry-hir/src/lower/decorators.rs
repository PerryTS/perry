//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

use perry_types::{FunctionType, Type};

use super::*;
use crate::ir::*;

pub(crate) fn append_legacy_decorator_init_for_class(
    ctx: &mut LoweringContext,
    init: &mut Vec<Stmt>,
    class: &Class,
) {
    let has_param_decorators = class
        .constructor
        .as_ref()
        .map(|ctor| ctor.params.iter().any(|p| !p.decorators.is_empty()))
        .unwrap_or(false);
    let has_method_decorators = class.methods.iter().any(method_has_legacy_decorators)
        || class
            .static_methods
            .iter()
            .any(method_has_legacy_decorators);
    let has_property_decorators = class.fields.iter().any(field_has_legacy_decorators)
        || class.static_fields.iter().any(field_has_legacy_decorators);
    if class.decorators.is_empty()
        && !has_param_decorators
        && !has_method_decorators
        && !has_property_decorators
    {
        return;
    }

    if let Some(ctor) = &class.constructor {
        let param_types = ctor
            .params
            .iter()
            .map(|p| type_metadata_expr(&p.ty))
            .collect();
        init.push(Stmt::Expr(Expr::ReflectDefineMetadata {
            key: Box::new(Expr::String("design:paramtypes".to_string())),
            value: Box::new(Expr::Array(param_types)),
            target: Box::new(Expr::ClassRef(class.name.clone())),
            property_key: None,
        }));
    }

    if let Some(ctor) = &class.constructor {
        for (index, param) in ctor.params.iter().enumerate().rev() {
            append_decorator_invocations(
                ctx,
                init,
                &param.decorators,
                vec![
                    Expr::ClassRef(class.name.clone()),
                    Expr::Undefined,
                    Expr::Number(index as f64),
                ],
            );
        }
    }

    for method in &class.methods {
        append_method_decorator_init(ctx, init, class, method);
    }
    for method in &class.static_methods {
        append_method_decorator_init(ctx, init, class, method);
    }
    for field in &class.fields {
        append_property_decorator_init(ctx, init, class, field);
    }
    for field in &class.static_fields {
        append_property_decorator_init(ctx, init, class, field);
    }

    append_class_decorator_invocations(
        ctx,
        init,
        &class.decorators,
        &class.name,
        vec![Expr::ClassRef(class.name.clone())],
    );
}

fn method_has_legacy_decorators(method: &Function) -> bool {
    !method.decorators.is_empty() || method.params.iter().any(|p| !p.decorators.is_empty())
}

fn field_has_legacy_decorators(field: &ClassField) -> bool {
    !field.decorators.is_empty()
}

fn append_property_decorator_init(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    class: &Class,
    field: &ClassField,
) {
    if field.decorators.is_empty() {
        return;
    }

    // NOTE: TypeScript's `emitDecoratorMetadata` stores instance-member
    // `design:type` on `Class.prototype`, and class-transformer reads it back
    // with `Reflect.getMetadata("design:type", SomeClass.prototype, prop)`.
    // Perry stores it on the class constructor (`ClassRef`) here; the runtime
    // metadata store folds class-prototype lookup targets onto the same class
    // key (see `proxy/metadata.rs::normalize_target_bits`), so both the
    // historical `getMetadata(..., Class, prop)` reads (locked in by
    // test_decorators_legacy_property_metadata.ts) and the library's
    // `Class.prototype` reads resolve to this entry.
    out.push(Stmt::Expr(Expr::ReflectDefineMetadata {
        key: Box::new(Expr::String("design:type".to_string())),
        value: Box::new(type_metadata_expr(&field.ty)),
        target: Box::new(Expr::ClassRef(class.name.clone())),
        property_key: Some(Box::new(Expr::String(field.name.clone()))),
    }));

    append_decorator_invocations(
        ctx,
        out,
        &field.decorators,
        vec![
            Expr::ClassRef(class.name.clone()),
            Expr::String(field.name.clone()),
        ],
    );
}

fn append_method_decorator_init(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    class: &Class,
    method: &Function,
) {
    if !method_has_legacy_decorators(method) {
        return;
    }

    if method.params.iter().any(|p| !p.decorators.is_empty()) || !method.decorators.is_empty() {
        let param_types = method
            .params
            .iter()
            .map(|p| type_metadata_expr(&p.ty))
            .collect();
        out.push(Stmt::Expr(Expr::ReflectDefineMetadata {
            key: Box::new(Expr::String("design:paramtypes".to_string())),
            value: Box::new(Expr::Array(param_types)),
            target: Box::new(Expr::ClassRef(class.name.clone())),
            property_key: Some(Box::new(Expr::String(method.name.clone()))),
        }));
    }

    for (index, param) in method.params.iter().enumerate().rev() {
        append_decorator_invocations(
            ctx,
            out,
            &param.decorators,
            vec![
                Expr::ClassRef(class.name.clone()),
                Expr::String(method.name.clone()),
                Expr::Number(index as f64),
            ],
        );
    }

    let descriptor = Expr::Object(vec![(
        "value".to_string(),
        Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_class_prototype_method_value".to_string(),
                param_types: Vec::new(),
                return_type: Type::Any,
            }),
            args: vec![
                Expr::ClassRef(class.name.clone()),
                Expr::String(method.name.clone()),
            ],
            type_args: Vec::new(),
            byte_offset: 0,
        },
    )]);
    append_decorator_invocations(
        ctx,
        out,
        &method.decorators,
        vec![
            Expr::ClassRef(class.name.clone()),
            Expr::String(method.name.clone()),
            descriptor,
        ],
    );
}

fn append_decorator_invocations(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    decorators: &[Decorator],
    invocation_args: Vec<Expr>,
) {
    append_decorator_invocations_inner(ctx, out, decorators, invocation_args, None);
}

/// Same as `append_decorator_invocations`, but each non-`Reflect.metadata`
/// invocation captures its return value and throws a `TypeError` if it is
/// anything other than `undefined`. Used for class decorators, where TS
/// allows the decorator to return a replacement class but Perry does not
/// install the replacement (the lowered class is fixed in the IR). Throwing
/// on a non-`undefined` return surfaces the silent-failure case the
/// maintainer flagged on PR #754 (`@Memoize`, `@Throttle`, GraphQL resolver
/// wrappers, etc.).
fn append_class_decorator_invocations(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    decorators: &[Decorator],
    class_name: &str,
    invocation_args: Vec<Expr>,
) {
    append_decorator_invocations_inner(ctx, out, decorators, invocation_args, Some(class_name));
}

fn append_decorator_invocations_inner(
    ctx: &mut LoweringContext,
    out: &mut Vec<Stmt>,
    decorators: &[Decorator],
    invocation_args: Vec<Expr>,
    class_name_for_replacement_check: Option<&str>,
) {
    let mut callees: Vec<(Expr, String)> = Vec::with_capacity(decorators.len());
    for dec in decorators {
        if dec.is_reflect_metadata {
            append_reflect_metadata_decorator(out, dec, &invocation_args);
            continue;
        }
        let base = decorator_callee_expr(ctx, &dec.name);
        if dec.is_factory {
            let temp_id = ctx.fresh_local();
            out.push(Stmt::Let {
                id: temp_id,
                name: format!("__perry_dec_{}", temp_id),
                ty: Type::Function(FunctionType {
                    params: Vec::new(),
                    return_type: Box::new(Type::Any),
                    is_async: false,
                    is_generator: false,
                }),
                mutable: false,
                init: Some(Expr::Call {
                    callee: Box::new(base),
                    args: dec.args.clone(),
                    type_args: Vec::new(),
                    byte_offset: 0,
                }),
            });
            callees.push((Expr::LocalGet(temp_id), dec.name.clone()));
        } else {
            callees.push((base, dec.name.clone()));
        }
    }

    for (callee, dec_name) in callees.into_iter().rev() {
        let call = Expr::Call {
            callee: Box::new(callee),
            args: invocation_args.clone(),
            type_args: Vec::new(),
            byte_offset: 0,
        };
        match class_name_for_replacement_check {
            Some(class_name) => {
                let ret_id = ctx.fresh_local();
                out.push(Stmt::Let {
                    id: ret_id,
                    name: format!("__perry_dec_ret_{}", ret_id),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(call),
                });
                let msg = format!(
                    "Class decorator `@{dec_name}` on `{class_name}` returned a value. \
Perry does not install decorator return values as class replacements (see \
docs/src/language/decorators.md). Return `undefined` (or nothing) to keep \
the decorator running for side effects only."
                );
                // Issue #832: also accept the identity return (`return target`)
                // as a no-op — node under `--experimental-strip-types` ignores
                // decorator return values entirely, and many real-world
                // decorators (factory output that hasn't been simplified, the
                // trivial `return target` no-op) hit this. Only the *actual*
                // class-replacement case (`return SomeOtherFn`) still throws.
                // The target is the first invocation arg (`Expr::ClassRef` for
                // class decorators); if it's not available, fall back to the
                // original strict undefined-only check.
                let ret_get = Expr::LocalGet(ret_id);
                let ne_undef = Expr::Compare {
                    op: CompareOp::Ne,
                    left: Box::new(ret_get.clone()),
                    right: Box::new(Expr::Undefined),
                };
                let condition = match invocation_args.first() {
                    Some(target_expr) => Expr::Logical {
                        op: LogicalOp::And,
                        left: Box::new(ne_undef),
                        right: Box::new(Expr::Compare {
                            op: CompareOp::Ne,
                            left: Box::new(ret_get),
                            right: Box::new(target_expr.clone()),
                        }),
                    },
                    None => ne_undef,
                };
                out.push(Stmt::If {
                    condition,
                    // Perry has dedicated HIR variants for built-in errors
                    // (`Expr::TypeErrorNew`, etc.); the generic
                    // `Expr::New { class_name: "TypeError" }` path falls
                    // through to an empty-object placeholder and prints
                    // `Uncaught exception: [object] (bits=…)`.
                    then_branch: vec![Stmt::Throw(Expr::TypeErrorNew(Box::new(Expr::String(msg))))],
                    else_branch: None,
                });
            }
            None => {
                out.push(Stmt::Expr(call));
            }
        }
    }
}

fn append_reflect_metadata_decorator(
    out: &mut Vec<Stmt>,
    dec: &Decorator,
    invocation_args: &[Expr],
) {
    let key = dec.args.first().cloned().unwrap_or(Expr::Undefined);
    let value = dec.args.get(1).cloned().unwrap_or(Expr::Undefined);
    let target = invocation_args.first().cloned().unwrap_or(Expr::Undefined);
    let property_key = invocation_args.get(1).cloned().and_then(|arg| {
        if matches!(arg, Expr::Undefined) {
            None
        } else {
            Some(Box::new(arg))
        }
    });
    out.push(Stmt::Expr(Expr::ReflectDefineMetadata {
        key: Box::new(key),
        value: Box::new(value),
        target: Box::new(target),
        property_key,
    }));
}

fn type_metadata_expr(ty: &Type) -> Expr {
    match ty {
        Type::Named(name) => Expr::ClassRef(name.clone()),
        Type::Generic { base, .. } => Expr::ClassRef(base.clone()),
        Type::Array(_) | Type::Tuple(_) => Expr::ClassRef("Array".to_string()),
        Type::String => Expr::ClassRef("String".to_string()),
        Type::Number | Type::Int32 | Type::BigInt => Expr::ClassRef("Number".to_string()),
        Type::Boolean => Expr::ClassRef("Boolean".to_string()),
        Type::Object(_) => Expr::ClassRef("Object".to_string()),
        Type::Function(_) => Expr::ClassRef("Function".to_string()),
        _ => Expr::Undefined,
    }
}

fn decorator_callee_expr(ctx: &LoweringContext, name: &str) -> Expr {
    if let Some(id) = ctx.lookup_func(name) {
        Expr::FuncRef(id)
    } else if let Some(orig_name) = ctx.lookup_imported_func(name) {
        let (param_types, return_type) = ctx
            .lookup_extern_func_types(orig_name)
            .map(|(p, r)| (p.clone(), r.clone()))
            .unwrap_or_else(|| (Vec::new(), Type::Any));
        Expr::ExternFuncRef {
            name: orig_name.to_string(),
            param_types,
            return_type,
        }
    } else {
        Expr::ExternFuncRef {
            name: name.to_string(),
            param_types: Vec::new(),
            return_type: Type::Any,
        }
    }
}

/// Emit a one-shot note when the user imports `reflect-metadata`. Perry
/// ships a built-in subset that's enough for the Nest-style decorator
/// metadata canaries (see docs/src/language/decorators.md), but is NOT
/// the full npm package's surface — `class-validator` and `TypeORM` reach
/// into corners that aren't shimmed. Telling the user up-front avoids the
/// silent-failure surprise where a polyfill they `import`-ed turns out
/// not to be the polyfill they expected.
pub(crate) fn emit_reflect_metadata_shim_note() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static EMITTED: AtomicBool = AtomicBool::new(false);
    if EMITTED.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "[perry] note: `import \"reflect-metadata\"` is satisfied by Perry's built-in \
metadata subset. Implemented surface: Reflect.defineMetadata, getMetadata, \
getOwnMetadata, hasMetadata, hasOwnMetadata, getMetadataKeys, \
getOwnMetadataKeys, deleteMetadata, and @Reflect.metadata(...). \
Anything outside this surface (Symbol-keyed metadata, the full reflect-metadata \
runtime used by class-validator/TypeORM) is not provided. \
See docs/src/language/decorators.md."
    );
}
