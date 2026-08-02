//! `Object` static-method calls.
//!
//! Extracted from `expr_call/native_module.rs` as a mechanical move; the
//! block runs in the same position it occupied inline, and `Ok(Err(args))`
//! means "not an `Object` static — keep checking the later receivers".

use super::*;

use anyhow::Result;
use swc_ecma_ast as ast;

pub(super) fn try_object_statics(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    member: &ast::MemberExpr,
    obj_name: &str,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for Object static methods. #6677: match BOTH the dot form
    // and the string-literal computed form (`Object["keys"](...)`) so the
    // computed key does not fall through to generic dispatch (→
    // `TypeError: value is not a function`).
    if obj_name == "Object" {
        if let Some(method_name) = super::super::static_call_prop_name(&member.prop) {
            match method_name {
                "keys" => {
                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectKeys(Box::new(obj))));
                }
                "values" => {
                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectValues(Box::new(obj))));
                }
                "entries" => {
                    let obj = args.first().cloned().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectEntries(Box::new(obj))));
                }
                // Object.assign(target, ...sources) — per ECMAScript spec, this
                // MUTATES target with each source's own enumerable string-keyed
                // and Symbol-keyed properties, and RETURNS target (preserving
                // identity, class_id, and the SYMBOL_PROPERTIES side-table).
                // Refs #590: the previous lowering folded the call into
                // ObjectSpread which allocates a fresh object — that breaks
                // `result === target` and orphans target's symbol-keyed
                // properties since the side table is keyed by raw pointer.
                //
                "assign" => {
                    let mut iter = args.into_iter();
                    let target = iter.next().unwrap_or(Expr::Undefined);
                    let sources: Vec<Expr> = iter.collect();
                    // Real `Object.assign(target, ...sources)` — mutate target.
                    return Ok(Ok(Expr::ObjectAssign {
                        target: Box::new(target),
                        sources,
                    }));
                }
                "fromEntries" => {
                    let entries = args.into_iter().next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectFromEntries(Box::new(entries))));
                }
                "groupBy"
                    // Object.groupBy(items, keyFn) — Node 22+ static method
                    if args.len() >= 2 => {
                        let mut iter = args.into_iter();
                        let items = iter.next().unwrap();
                        let key_fn = iter.next().unwrap();
                        let key_fn = ctx.maybe_wrap_builtin_callback(key_fn, &call.args[1]);
                        return Ok(Ok(Expr::ObjectGroupBy {
                            items: Box::new(items),
                            key_fn: Box::new(key_fn),
                        }));
                    }
                "is" => {
                    let mut iter = args.into_iter();
                    let a = iter.next().unwrap_or(Expr::Undefined);
                    let b = iter.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectIs(Box::new(a), Box::new(b))));
                }
                "hasOwn" => {
                    let mut iter = args.into_iter();
                    let obj = iter.next().unwrap_or(Expr::Undefined);
                    let key = iter.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectHasOwn(Box::new(obj), Box::new(key))));
                }
                "freeze" => {
                    return Ok(Ok(Expr::ObjectFreeze(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "seal" => {
                    return Ok(Ok(Expr::ObjectSeal(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "preventExtensions" => {
                    return Ok(Ok(Expr::ObjectPreventExtensions(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "create" => {
                    let mut it = args.into_iter();
                    let proto = it.next().unwrap_or(Expr::Undefined);
                    let props = it.next().map(Box::new);
                    return Ok(Ok(Expr::ObjectCreate(Box::new(proto), props)));
                }
                "isFrozen" => {
                    return Ok(Ok(Expr::ObjectIsFrozen(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "isSealed" => {
                    return Ok(Ok(Expr::ObjectIsSealed(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "isExtensible" => {
                    return Ok(Ok(Expr::ObjectIsExtensible(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "getPrototypeOf" => {
                    return Ok(Ok(Expr::ObjectGetPrototypeOf(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "setPrototypeOf" => {
                    // `Object.setPrototypeOf(obj, proto)` is the foundation
                    // of chalk's "callable + getter-bag" shape (a closure has
                    // its `[[Prototype]]` reset to a Function-derived
                    // accessor-bag). Pre-fix this fell through to a generic
                    // `Object.setPrototypeOf` PropertyGet → Call where
                    // `Object.setPrototypeOf` resolves to undefined and the
                    // call throws `TypeError: value is not a function` —
                    // chalk's `import chalk from "chalk"` died at module init.
                    //
                    // Perry's runtime doesn't track mutable per-instance
                    // prototype chains (class IDs are baked at allocation),
                    // so we model setPrototypeOf as a no-op that still
                    // returns the target — matching the spec's "return obj"
                    // contract. The runtime helper registers (obj, proto)
                    // in a side-table that `Object.getPrototypeOf(obj)` is
                    // free to consult later if a downstream pattern needs it.
                    let mut iter = args.into_iter();
                    let obj = iter.next().unwrap_or(Expr::Undefined);
                    let proto = iter.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectSetPrototypeOf(
                        Box::new(obj),
                        Box::new(proto),
                    )));
                }
                "defineProperty" => {
                    let mut iter = args.into_iter();
                    let obj = iter.next().unwrap_or(Expr::Undefined);
                    let key = iter.next().unwrap_or(Expr::Undefined);
                    let descriptor = iter.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectDefineProperty(
                        Box::new(obj),
                        Box::new(key),
                        Box::new(descriptor),
                    )));
                }
                "defineProperties" => {
                    // `Object.defineProperties(target, descriptors)` — bulk
                    // form of `defineProperty`. Used by chalk's index.js to
                    // attach the `styles` getter-bag onto
                    // `createChalk.prototype`. Pre-fix this fell through to a
                    // generic `(Object).defineProperties(...)` call which
                    // throws `TypeError: value is not a function` at module
                    // init because `Object` isn't a runtime object with
                    // method dispatch.
                    //
                    // Desugar to a sequence of `ObjectDefineProperty`
                    // applications by reading `descriptors`'s own keys at
                    // compile time when it's an object literal, otherwise
                    // route through a runtime helper that iterates the
                    // descriptor object's keys.
                    let mut iter = args.into_iter();
                    let target = iter.next().unwrap_or(Expr::Undefined);
                    let descs = iter.next().unwrap_or(Expr::Undefined);
                    if let Expr::Object(props) = &descs {
                        // Static descriptor literal — desugar to a Sequence
                        // of `defineProperty(target, key, desc)` calls and
                        // yield `target` as the result value.
                        //
                        // An EMPTY literal must NOT fold to a bare `target`:
                        // `Object.defineProperties(O, {})` still performs the
                        // spec's step-1 `If Type(O) is not Object, throw a
                        // TypeError`, so `Object.defineProperties(undefined,
                        // {})` must throw. With no keys there is no per-key
                        // `defineProperty` to enforce that, so route the
                        // empty case through the runtime helper (which
                        // validates the target).
                        if !props.is_empty() {
                            let target = target;
                            let mut exprs: Vec<Expr> = Vec::with_capacity(props.len() + 1);
                            for (key_name, desc_expr) in props {
                                exprs.push(Expr::ObjectDefineProperty(
                                    Box::new(target.clone()),
                                    Box::new(Expr::String(key_name.clone())),
                                    Box::new(desc_expr.clone()),
                                ));
                            }
                            exprs.push(target);
                            return Ok(Ok(Expr::Sequence(exprs)));
                        }
                    }
                    return Ok(Ok(Expr::ObjectDefineProperties(
                        Box::new(target),
                        Box::new(descs),
                    )));
                }
                "getOwnPropertyDescriptor" => {
                    // #2144/#3655: built-in function `.name` /
                    // `.length` descriptors.
                    //
                    // `Object.getOwnPropertyDescriptor(<BuiltinCtor>,
                    // "name"|"length")` and
                    // `…(<BuiltinNs>.<staticFn>, "name"|"length")`
                    // need a compile-time fold because those builtin
                    // values are often intrinsic sentinels rather than
                    // first-class closures. Per spec both descriptors
                    // are non-writable, non-enumerable, configurable
                    // data properties. Fold when we can statically
                    // recognize the receiver shape — same gating logic
                    // as the direct `.name` / `.length` folds in
                    // `expr_member.rs`.
                    if call.args.len() >= 2 && args.len() >= 2 {
                        let key_name = match call.args[1].expr.as_ref() {
                            ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                            _ => None,
                        };
                        if matches!(key_name, Some("name" | "length")) {
                            let lowered_obj_is_global_intrinsic = match &args[0] {
                                Expr::GlobalGet(0) => true,
                                Expr::PropertyGet { object: inner, .. } => {
                                    matches!(inner.as_ref(), Expr::GlobalGet(0))
                                }
                                _ => false,
                            };
                            if lowered_obj_is_global_intrinsic {
                                match key_name {
                                    Some("name") => {
                                        let folded =
                                            super::super::name_fold::builtin_fn_name_for_arg(
                                                call.args[0].expr.as_ref(),
                                            );
                                        if let Some(fname) = folded {
                                            return Ok(Ok(
                                                super::super::name_fold::name_data_descriptor(
                                                    fname,
                                                ),
                                            ));
                                        }
                                    }
                                    Some("length") => {
                                        let folded =
                                            super::super::name_fold::builtin_fn_length_for_arg(
                                                call.args[0].expr.as_ref(),
                                            )
                                            .or_else(|| {
                                                super::super::name_fold::builtin_fn_name_for_arg(
                                                    call.args[0].expr.as_ref(),
                                                )
                                                .and_then(|name| {
                                                    crate::analysis::builtin_constructor_length(
                                                        &name,
                                                    )
                                                })
                                            });
                                        if let Some(len) = folded {
                                            return Ok(Ok(
                                                super::super::name_fold::builtin_data_descriptor(
                                                    Expr::Number(len as f64),
                                                ),
                                            ));
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    let mut iter = args.into_iter();
                    let obj = iter.next().unwrap_or(Expr::Undefined);
                    let key = iter.next().unwrap_or(Expr::Undefined);
                    return Ok(Ok(Expr::ObjectGetOwnPropertyDescriptor(
                        Box::new(obj),
                        Box::new(key),
                    )));
                }
                "getOwnPropertyDescriptors" => {
                    return Ok(Ok(Expr::ObjectGetOwnPropertyDescriptors(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "getOwnPropertyNames" => {
                    return Ok(Ok(Expr::ObjectGetOwnPropertyNames(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                "getOwnPropertySymbols" => {
                    return Ok(Ok(Expr::ObjectGetOwnPropertySymbols(Box::new(
                        args.into_iter().next().unwrap_or(Expr::Undefined),
                    ))));
                }
                _ => {} // Fall through to generic handling
            }
        }
    }

    Ok(Err(args))
}
