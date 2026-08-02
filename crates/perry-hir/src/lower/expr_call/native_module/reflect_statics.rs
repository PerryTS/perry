//! `Reflect` static-method calls.
//!
//! Extracted from `expr_call/native_module.rs` as a mechanical move; the
//! block runs in the same position it occupied inline, and `Ok(Err(args))`
//! means "not a `Reflect` static — keep checking the later receivers".

use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

pub(super) fn try_reflect_statics(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    member: &ast::MemberExpr,
    obj_name: &str,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    if obj_name == "Reflect" {
        // #6677: accept the string-literal computed form too.
        if let Some(method_name) = super::super::static_call_prop_name(&member.prop) {
            match method_name {
                "get" => {
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let key = it.next().unwrap_or(Expr::Undefined);
                    // #2766: optional `receiver` (3rd arg) used as the
                    // `this` binding for accessor getters. Default to
                    // `undefined` — the runtime substitutes `target`.
                    let receiver = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectGet {
                        target: Box::new(target),
                        key: Box::new(key),
                        receiver: Box::new(receiver),
                    }));
                }
                "set" => {
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let key = it.next().unwrap_or(Expr::Undefined);
                    let value = it.next().unwrap_or(Expr::Undefined);
                    // Optional `receiver` (4th arg): default `undefined`
                    // and the runtime substitutes `target`.
                    let receiver = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectSet {
                        target: Box::new(target),
                        key: Box::new(key),
                        value: Box::new(value),
                        receiver: Box::new(receiver),
                    }));
                }
                "has" => {
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let key = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectHas {
                        target: Box::new(target),
                        key: Box::new(key),
                    }));
                }
                "deleteProperty" => {
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let key = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectDelete {
                        target: Box::new(target),
                        key: Box::new(key),
                    }));
                }
                "ownKeys" => {
                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectOwnKeys(Box::new(target))));
                }
                "apply" => {
                    let mut it = args.into_iter();
                    let func = it.next().unwrap_or(Expr::Undefined);
                    let this_arg = it.next().unwrap_or(Expr::Undefined);
                    // Spec sec-reflect.apply runs CreateListFromArrayLike
                    // on argumentsList, which throws a TypeError when
                    // Type(argumentsList) is not Object. An OMITTED
                    // argumentsList is `undefined` (not Object), so it
                    // must reach the runtime as `undefined` and throw —
                    // NOT default to an empty array, which would silently
                    // succeed with no args (test262
                    // Reflect/apply/arguments-list-is-not-array-like
                    // `Reflect.apply(fn, null /* empty */)`).
                    let args_arr = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectApply {
                        func: Box::new(func),
                        this_arg: Box::new(this_arg),
                        args: Box::new(args_arr),
                    }));
                }
                "construct" => {
                    // Special case: `Reflect.construct(ClassName, [args...])`
                    // where ClassName is a known class — fold to a direct
                    // `new ClassName(...args)` expression.
                    //
                    // #2768: only fold the two-argument form. With an
                    // explicit `newTarget` (3rd arg) the result's prototype
                    // comes from `newTarget` and `newTarget` must be
                    // validated as a constructor — a plain `new ClassName`
                    // would silently drop both. Fall through to
                    // `ReflectConstruct` (runtime `js_reflect_construct`,
                    // which runs `js_new_function_construct_with_new_target`
                    // and the non-constructor `newTarget` TypeError check).
                    if call.args.len() == 2 {
                        if let ast::Expr::Ident(cls_ident) = call.args[0].expr.as_ref() {
                            let cls_name = cls_ident.sym.to_string();
                            if ctx.lookup_class(&cls_name).is_some() {
                                if let ast::Expr::Array(arr_lit) = call.args[1].expr.as_ref() {
                                    let new_args: Vec<Expr> = arr_lit
                                        .elems
                                        .iter()
                                        .filter_map(|e| e.as_ref())
                                        .map(|e| lower_expr(ctx, &e.expr))
                                        .collect::<Result<Vec<_>>>()?;
                                    return Ok(Ok(Expr::New {
                                        class_name: cls_name,
                                        args: new_args,
                                        type_args: vec![],
                                        byte_offset: 0,
                                        cap_args_appended: 0,
                                    }));
                                }
                            }
                        }
                    }
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let args_arr = it.next().unwrap_or(Expr::Array(vec![]));
                    // 3rd arg = newTarget; defaults to `undefined` so the
                    // runtime falls back to the target/proxy itself.
                    let new_target = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectConstruct {
                        target: Box::new(target),
                        args: Box::new(args_arr),
                        new_target: Box::new(new_target),
                    }));
                }
                "defineProperty" => {
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let key = it.next().unwrap_or(Expr::Undefined);
                    let descriptor = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectDefineProperty {
                        target: Box::new(target),
                        key: Box::new(key),
                        descriptor: Box::new(descriptor),
                    }));
                }
                "getPrototypeOf" => {
                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectGetPrototypeOf(Box::new(target))));
                }
                "getOwnPropertyDescriptor" => {
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let key = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectGetOwnPropertyDescriptor {
                        target: Box::new(target),
                        key: Box::new(key),
                    }));
                }
                "defineMetadata" => {
                    let (key, value, target, property_key) = take_reflect_kvtp_args(args);
                    return Ok(Ok(Expr::ReflectDefineMetadata {
                        key: Box::new(key),
                        value: Box::new(value),
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "getMetadata" => {
                    let (key, target, property_key) = take_reflect_ktp_args(args);
                    return Ok(Ok(Expr::ReflectGetMetadata {
                        key: Box::new(key),
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "getOwnMetadata" => {
                    let (key, target, property_key) = take_reflect_ktp_args(args);
                    return Ok(Ok(Expr::ReflectGetOwnMetadata {
                        key: Box::new(key),
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "hasMetadata" => {
                    let (key, target, property_key) = take_reflect_ktp_args(args);
                    return Ok(Ok(Expr::ReflectHasMetadata {
                        key: Box::new(key),
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "hasOwnMetadata" => {
                    let (key, target, property_key) = take_reflect_ktp_args(args);
                    return Ok(Ok(Expr::ReflectHasOwnMetadata {
                        key: Box::new(key),
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "getMetadataKeys" => {
                    let (target, property_key) = take_reflect_tp_args(args);
                    return Ok(Ok(Expr::ReflectGetMetadataKeys {
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "getOwnMetadataKeys" => {
                    let (target, property_key) = take_reflect_tp_args(args);
                    return Ok(Ok(Expr::ReflectGetOwnMetadataKeys {
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "deleteMetadata" => {
                    let (key, target, property_key) = take_reflect_ktp_args(args);
                    return Ok(Ok(Expr::ReflectDeleteMetadata {
                        key: Box::new(key),
                        target: Box::new(target),
                        property_key,
                    }));
                }
                "setPrototypeOf" => {
                    // #2761: Reflect-specific — boolean result (false on
                    // rejected change) + TypeError on bad args, distinct
                    // from Object.setPrototypeOf (returns the object).
                    let mut it = args.into_iter();
                    let target = it.next().unwrap_or(Expr::Undefined);
                    let proto = it.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectSetPrototypeOf {
                        target: Box::new(target),
                        proto: Box::new(proto),
                    }));
                }
                "isExtensible" => {
                    // #2762: Reflect-specific semantics (boolean +
                    // TypeError on non-object), NOT Object.isExtensible.
                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectIsExtensible(Box::new(target))));
                }
                "preventExtensions" => {
                    // #2762: Reflect-specific semantics (boolean +
                    // TypeError on non-object), NOT Object.preventExtensions.
                    let target = args.into_iter().next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ReflectPreventExtensions(Box::new(target))));
                }
                _ => {}
            }
        }
    }

    Ok(Err(args))
}
