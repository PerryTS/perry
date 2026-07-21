//! The `ast::Expr::OptChain` arm of `lower_expr_impl`, extracted to a helper.
//! Pure code move — no behavior change.

use super::*;
use anyhow::Result;
use swc_ecma_ast as ast;

pub(crate) fn lower_opt_chain_expr(
    ctx: &mut LoweringContext,
    opt_chain: &ast::OptChainExpr,
) -> Result<Expr> {
    // Optional chaining: obj?.prop or obj?.[index] or obj?.method()
    // Convert to: obj == null ? undefined : obj.prop
    match &*opt_chain.base {
        ast::OptChainBase::Member(member) => {
            // Issue #449: `new.target?.<prop>` folds to a literal at
            // lowering time — same shape as the direct
            // `new.target.<prop>` fold in `expr_member::lower_member`,
            // applied here BEFORE `lower_expr(&member.obj)` would
            // otherwise route MetaProp(NewTarget) through the
            // broken Object-literal synthesis path. Inside a
            // constructor `new.target` is non-null/non-undefined,
            // so the optional chain just resolves the property;
            // outside a constructor it's undefined and the chain
            // short-circuits.
            if let ast::Expr::MetaProp(mp) = member.obj.as_ref() {
                if matches!(mp.kind, ast::MetaPropKind::NewTarget) {
                    if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                        let prop_name = prop_ident.sym.as_ref();
                        // #2768: `new.target?.<prop>` reads off the
                        // runtime new.target (a leaf class ref inside a
                        // constructor, `undefined` outside). Inside a
                        // ctor it's non-null so `?.` resolves the
                        // property; outside it yields undefined. The old
                        // fold hardcoded the enclosing class name (wrong
                        // leaf) and undefined for `.prototype`.
                        return Ok(Expr::PropertyGet {
                            byte_offset: 0,
                            object: Box::new(Expr::NewTarget),
                            property: prop_name.to_string(),
                        });
                    }
                }
            }
            // #6719: `Symbol?.iterator` (and `Symbol?.["iterator"]` /
            // `Symbol?.[name]`) must resolve the well-known symbol, exactly like
            // the non-optional `Symbol.iterator` / `Symbol["iterator"]` (#6676)
            // forms — the dot/bracket folds live in `lower_member_inner`, which
            // this optional-chain arm never reaches, so without this a
            // well-known symbol read via `?.` fell through to a generic property
            // get on the `Symbol` constructor and returned `undefined` (a class
            // keyed with `[Symbol?.iterator]` was then not iterable). `Symbol` is
            // a non-nullish global, so `Symbol?.iterator === Symbol.iterator` and
            // the `?.` short-circuit is dead — the resolver returns the fold
            // directly.
            if let Some(folded) = expr_member::try_fold_symbol_well_known_member(
                ctx,
                member.obj.as_ref(),
                &member.prop,
            )? {
                return Ok(folded);
            }
            // obj?.prop -> obj == null ? undefined : obj.prop
            let obj_expr = lower_expr(ctx, &member.obj)?;

            // Get the property access
            let prop_expr = match &member.prop {
                ast::MemberProp::Ident(ident) => {
                    let prop_name = ident.sym.to_string();
                    // RegExp exec/match `.index` / `.groups` / `.input`
                    // are real own properties on the result array
                    // (regex.rs), so they resolve as a generic
                    // PropertyGet — no thread-local fold. This keeps a
                    // stored result correct after an intervening match
                    // on another regex.
                    Expr::PropertyGet {
                        byte_offset: 0,
                        object: Box::new(obj_expr.clone()),
                        property: prop_name,
                    }
                }
                ast::MemberProp::Computed(comp) => {
                    let index = lower_expr(ctx, &comp.expr)?;
                    Expr::IndexGet {
                        object: Box::new(obj_expr.clone()),
                        index: Box::new(index),
                    }
                }
                ast::MemberProp::PrivateName(private) => {
                    let property = format!("#{}", private.name);
                    let object = expr_member::wrap_private_guard(
                        ctx,
                        Box::new(obj_expr.clone()),
                        &property,
                        expr_member::PRIV_OP_READ,
                    );
                    Expr::PropertyGet {
                        byte_offset: 0,
                        object,
                        property,
                    }
                }
            };

            // Issue #388: optional chaining short-circuits on
            // null OR undefined per spec. Use `LooseEq` so the
            // comparison `obj == null` matches both — strict
            // `===` only matches null, leaving undefined to
            // fall through and dereference (returning
            // `[object Object]` for Map.get's missing value).
            Ok(Expr::Conditional {
                condition: Box::new(Expr::Compare {
                    op: CompareOp::LooseEq,
                    left: Box::new(obj_expr),
                    right: Box::new(Expr::Null),
                }),
                then_expr: Box::new(Expr::Undefined),
                else_expr: Box::new(prop_expr),
            })
        }
        ast::OptChainBase::Call(call) => {
            // OptChain(Call) is `<expr>?.(args)` — the `?.` is between the
            // callee and the call parens (e.g. `obj.method?.(args)`), NOT
            // `obj?.method(args)` (which SWC parses as Call(OptChain(Member))
            // and is handled via the regular Call lowering path).
            //
            // So the short-circuit must check the *function value* (the
            // callee), not the receiver. Issue #830: previously this
            // checked `obj == null`, which crashed when `obj.method` was
            // undefined while `obj` itself was a valid object.
            let callee = &call.callee;

            // Check for spread arguments
            let has_spread = call.args.iter().any(|arg| arg.spread.is_some());

            let args = call
                .args
                .iter()
                .map(|arg| lower_expr(ctx, &arg.expr))
                .collect::<Result<Vec<_>>>()?;

            // Lower callee as plain MemberExpr, unwrapping inner OptChain.
            // SWC may wrap the callee member access in an OptChain too.
            // We must NOT re-lower via lower_expr which would nest Conditionals.
            //
            // `callee_from_chain` records the `foo?.bar?.(args)` shape: the
            // callee is itself an optional chain, so `check_expr` is the
            // *receiver* (`foo`) rather than the function value. In that
            // case the receiver short-circuit alone is not enough — the
            // function value (`foo.bar`) must ALSO be null-checked before
            // the call, or an `undefined` property is invoked and throws
            // "X is not a function" (issue #4699: zod `safeParse`'s
            // `iss.inst?._zod.def?.error?.(iss)` error-map probe).
            let mut callee_from_chain = false;
            // True when the CALLEE's member access itself is optional
            // (`recv?.method(args)` — the `?.` before the method name),
            // as opposed to `callee_from_chain` which tracks the optional
            // CALL token (`recv.method?.(args)`). Needed for the
            // inner-Conditional nesting path below: when the receiver is
            // produced by an upstream optional chain (`a?.b?.method(args)`)
            // its lowered form is itself a Conditional, so the standard
            // receiver null-guard (built on the non-Conditional path) is
            // skipped — leaving `(a.b).method(args)` to dereference an
            // `undefined` receiver and throw "reading 'method'" instead of
            // short-circuiting (the `a?.b?.some(...)` wall). This flag lets
            // the nesting branch re-add that receiver guard.
            let mut opt_member_chain = false;
            // Receiver of an `obj.method?.(args)` callee, captured so the
            // function-value nullish guard can avoid false-short-circuiting
            // on string builtins (`type?.split?.(...)`) — see
            // `opt_call_func_nullish_guard`. `None` for non-member callees.
            let mut opt_call_member_receiver: Option<Expr> = None;
            let (check_expr, callee_expr) = {
                let mut lower_member_flat = |member: &ast::MemberExpr| -> Result<(Expr, Expr)> {
                    let obj = lower_expr(ctx, &member.obj)?;
                    let prop = match &member.prop {
                        ast::MemberProp::Ident(id) => Expr::PropertyGet {
                            byte_offset: 0,
                            object: Box::new(obj.clone()),
                            property: id.sym.to_string(),
                        },
                        ast::MemberProp::Computed(c) => {
                            let idx = lower_expr(ctx, &c.expr)?;
                            Expr::IndexGet {
                                object: Box::new(obj.clone()),
                                index: Box::new(idx),
                            }
                        }
                        ast::MemberProp::PrivateName(private) => {
                            let property = format!("#{}", private.name);
                            let guarded = expr_member::wrap_private_guard(
                                ctx,
                                Box::new(obj.clone()),
                                &property,
                                expr_member::PRIV_OP_READ,
                            );
                            Expr::PropertyGet {
                                byte_offset: 0,
                                object: guarded,
                                property,
                            }
                        }
                    };
                    Ok((obj, prop))
                };
                match &**callee {
                    // Simple `obj.method?.(args)`: check the function value
                    // (prop), call the function (prop) — codegen still sees
                    // a PropertyGet callee so `this` binds to obj.
                    ast::Expr::Member(m) => {
                        let (obj, prop) = lower_member_flat(m)?;
                        opt_call_member_receiver = Some(obj);
                        (prop.clone(), prop)
                    }
                    ast::Expr::OptChain(inner) => match &*inner.base {
                        // The callee is itself an optional chain. Two
                        // distinct shapes land here, told apart by whether
                        // THIS chain link's call is optional
                        // (`opt_chain.optional`, the `?.(` token):
                        //
                        //  • `foo?.bar?.(args)` (optional call): check the
                        //    receiver (foo) so the inner `?.` short-circuit
                        //    works, AND flag that the function value
                        //    (foo.bar) needs its own null-check before the
                        //    call (#4699 — an `undefined` property must
                        //    short-circuit, not throw "X is not a function").
                        //
                        //  • `foo?.bar(args)` (non-optional call, only the
                        //    member is optional): this is an ordinary method
                        //    call guarded by the receiver. It must NOT get a
                        //    function-value guard — `s?.at(-1)` reads `s.at`
                        //    as a bare PropertyGet, which is `undefined` for
                        //    builtin (string/array) methods that only resolve
                        //    through the call path, so the guard would wrongly
                        //    short-circuit the whole call (#4814). Leaving
                        //    `callee_from_chain` false yields the plain
                        //    `recv == null ? undefined : recv.method(args)`,
                        //    and codegen binds `this` from the PropertyGet
                        //    callee + dispatches the builtin normally.
                        ast::OptChainBase::Member(m) => {
                            callee_from_chain = opt_chain.optional;
                            // `inner.optional` is the `?.` on the method
                            // member itself (`recv?.method`). Capture it so
                            // the inner-Conditional nesting path can guard
                            // the receiver when it is `undefined`.
                            opt_member_chain = inner.optional;
                            let (obj, prop) = lower_member_flat(m)?;
                            opt_call_member_receiver = Some(obj.clone());
                            (obj, prop)
                        }
                        _ => {
                            let ce = lower_expr(ctx, callee)?;
                            (ce.clone(), ce)
                        }
                    },
                    _ => {
                        let ce = lower_expr(ctx, callee)?;
                        (ce.clone(), ce)
                    }
                }
            };

            // If check_expr is already a Conditional from an inner optional chain,
            // nest the outer call inside its else branch instead of creating another Conditional.
            // This avoids duplicating side-effecting expressions (like ArrayShift/ArrayPop).
            if let Expr::Conditional {
                condition: inner_cond,
                then_expr: inner_then,
                else_expr: inner_else,
            } = check_expr
            {
                // The receiver of the method call is `inner_else` (the
                // un-short-circuited result of the upstream chain, e.g.
                // `a.b` for `a?.b?.method(args)`). Keep a copy so an
                // optional method member (`?.method`) can null-guard it.
                // Captured whenever the method member is optional and the
                // receiver is side-effect-free (it appears twice: in the
                // guard and in the call). Used by BOTH the optional-call
                // (`?.method?.(args)`) and plain-call (`?.method(args)`)
                // branches — in the optional-call branch the function-value
                // nullish guard would otherwise read `(a.b).method` off a
                // null/undefined `a.b` and throw before short-circuiting.
                let receiver_for_member_guard =
                    if opt_member_chain && opt_call_receiver_repeatable(&inner_else) {
                        Some(inner_else.as_ref().clone())
                    } else {
                        None
                    };
                // Build the callee with inner_else as the object (not the full Conditional)
                let fixed_callee = match callee_expr {
                    Expr::PropertyGet { property, .. } => Expr::PropertyGet {
                        byte_offset: 0,
                        object: inner_else,
                        property,
                    },
                    Expr::IndexGet { index, .. } => Expr::IndexGet {
                        object: inner_else,
                        index,
                    },
                    other => other,
                };
                let outer_call = Expr::Call {
                    callee: Box::new(fixed_callee.clone()),
                    args,
                    type_args: Vec::new(),
                    byte_offset: 0,
                };
                // For `foo?.bar?.(args)` the function value (`bar` on the
                // un-short-circuited receiver) must itself be null-checked
                // before calling — otherwise an `undefined` property is
                // invoked and throws "X is not a function" (#4699).
                let else_expr: Box<Expr> = if callee_from_chain {
                    // String-builtin-safe nullish guard: a real string
                    // receiver never short-circuits even though
                    // `string.method` reads as undefined.
                    let guard_cond = match &opt_call_member_receiver {
                        Some(recv) => opt_call_func_nullish_guard(recv, fixed_callee),
                        None => Expr::Compare {
                            op: CompareOp::LooseEq,
                            left: Box::new(fixed_callee),
                            right: Box::new(Expr::Null),
                        },
                    };
                    let guarded_call = Expr::Conditional {
                        condition: Box::new(guard_cond),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(outer_call),
                    };
                    // `a?.b?.method?.(args)` — the function-value guard above
                    // reads `(a.b).method`, which THROWS when the upstream
                    // `a.b` is null/undefined (it lowered to a Conditional, so
                    // the receiver here is the un-short-circuited `a.b`). Wrap
                    // it in an outer receiver-nullish short-circuit so the
                    // chain returns `undefined` instead of throwing
                    // "Cannot read properties of <nullish> (reading 'method')".
                    match receiver_for_member_guard {
                        Some(recv) => Box::new(Expr::Conditional {
                            condition: Box::new(Expr::Compare {
                                op: CompareOp::LooseEq,
                                left: Box::new(recv),
                                right: Box::new(Expr::Null),
                            }),
                            then_expr: Box::new(Expr::Undefined),
                            else_expr: Box::new(guarded_call),
                        }),
                        None => Box::new(guarded_call),
                    }
                } else if let Some(recv) = receiver_for_member_guard {
                    // `a?.b?.method(args)` — the method member (`?.method`)
                    // is optional and its receiver (`a.b`) comes from an
                    // upstream optional chain, so it lowered to a Conditional
                    // and this branch lost the per-receiver null-guard that
                    // the non-Conditional path applies. Re-add it: if the
                    // RECEIVER is nullish, short-circuit to undefined instead
                    // of reading `.method` off `undefined` and throwing
                    // "Cannot read properties of undefined (reading 'method')"
                    // (the `a?.b?.some(...)` wall). The guard tests the
                    // receiver value directly — NOT the function value
                    // `(a.b).method`, which would itself throw while reading
                    // `.method` off the `undefined` receiver during guard
                    // evaluation. The receiver appears twice (guard + call),
                    // so this is only reached when it is side-effect-free
                    // (`receiver_for_member_guard` is None otherwise).
                    let guard_cond = Expr::Compare {
                        op: CompareOp::LooseEq,
                        left: Box::new(recv),
                        right: Box::new(Expr::Null),
                    };
                    Box::new(Expr::Conditional {
                        condition: Box::new(guard_cond),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(outer_call),
                    })
                } else {
                    Box::new(outer_call)
                };
                return Ok(Expr::Conditional {
                    condition: inner_cond,
                    then_expr: inner_then,
                    else_expr,
                });
            }

            // Keep the function value for the `foo?.bar?.(args)` guard
            // (see callee_from_chain) before it is moved into the call.
            let func_value_for_guard = if callee_from_chain {
                Some(callee_expr.clone())
            } else {
                None
            };

            // Build the call expression
            let call_expr = if has_spread {
                let spread_args: Vec<CallArg> = call
                    .args
                    .iter()
                    .zip(args.iter())
                    .map(|(ast_arg, lowered)| {
                        if ast_arg.spread.is_some() {
                            CallArg::Spread(lowered.clone())
                        } else {
                            CallArg::Expr(lowered.clone())
                        }
                    })
                    .collect();
                Expr::CallSpread {
                    callee: Box::new(callee_expr),
                    args: spread_args,
                    type_args: Vec::new(),
                }
            } else {
                // Try to fold known array methods (`.map`/`.filter`/etc.)
                // into their dedicated HIR variants here, since the regular
                // `lower_expr` Call array fast-path is on the AST CallExpr
                // path and never sees the synthetic Expr::Call we build
                // for `obj?.method(args)`.
                try_fold_array_method_call(Expr::Call {
                    callee: Box::new(callee_expr),
                    args,
                    type_args: Vec::new(),
                    byte_offset: 0,
                })
            };

            // For `foo?.bar?.(args)` the receiver check below guards `foo`,
            // but the function value `foo.bar` must ALSO be null-checked
            // before the call — otherwise an `undefined` property is
            // invoked and throws "X is not a function" (#4699).
            let else_expr: Box<Expr> = match func_value_for_guard {
                Some(func_value) => {
                    // String-builtin-safe: do not short-circuit when the
                    // receiver is a primitive string whose builtin method
                    // reads back as `undefined` (`type?.split?.(...)`).
                    let guard_cond = match &opt_call_member_receiver {
                        Some(recv) => opt_call_func_nullish_guard(recv, func_value),
                        None => Expr::Compare {
                            op: CompareOp::LooseEq,
                            left: Box::new(func_value),
                            right: Box::new(Expr::Null),
                        },
                    };
                    Box::new(Expr::Conditional {
                        condition: Box::new(guard_cond),
                        then_expr: Box::new(Expr::Undefined),
                        else_expr: Box::new(call_expr),
                    })
                }
                None => Box::new(call_expr),
            };

            // Issue #388: optional chaining short-circuits on
            // null OR undefined per spec. Use `LooseEq` so the
            // comparison `check_expr == null` matches both —
            // strict `===` only matches null, leaving
            // undefined to fall through and produce
            // `[object Object]` (or worse) when the receiver
            // is `Map.get(missing)` etc.
            //
            // For the simple `obj.method?.(args)` shape (`callee_from_chain`
            // is false and we captured a member receiver), `check_expr` is
            // the FUNCTION VALUE `obj.method`. Reading `string.method` as a
            // property yields `undefined` for builtins even though they're
            // callable, so use the string-builtin-safe guard to avoid a
            // false short-circuit (`"a/b".split?.(...)`). Otherwise
            // (`check_expr` is a receiver, or callee is not a member) the
            // plain nullish check is correct.
            let condition = if !callee_from_chain && opt_call_member_receiver.is_some() {
                let recv = opt_call_member_receiver.unwrap();
                opt_call_func_nullish_guard(&recv, check_expr)
            } else {
                Expr::Compare {
                    op: CompareOp::LooseEq,
                    left: Box::new(check_expr),
                    right: Box::new(Expr::Null),
                }
            };
            Ok(Expr::Conditional {
                condition: Box::new(condition),
                then_expr: Box::new(Expr::Undefined),
                else_expr,
            })
        }
    }
}
