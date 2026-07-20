//! Array methods on inline array literals (e.g., `['a','b'].join('-')`).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::{lower_expr, LoweringContext};

pub(super) fn try_inline_array_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for array methods on inline array literals (e.g., ['a', 'b'].join('-'))
    if let ast::Callee::Expr(expr) = &call.callee {
        if let ast::Expr::Member(member) = expr.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.as_ref();
                // #6718: the `args` vector these fast paths receive holds the
                // EVALUATED spread expression with the spread token dropped, so
                // any arm that reads a positional slot (`args[0]` as the callback,
                // comparator, index, search value, …) would bind the spread SOURCE
                // array itself — `[1,2].map(...[fn])` → "object is not a function",
                // `[1,2].slice(...[1,3])` → the whole array. Decline on ANY spread
                // so the generic tail builds an `Expr::CallSpread`, whose
                // member-callee arm flattens the spread into one argument array and
                // dispatches the array method by name with `this` bound to the
                // receiver (`js_native_call_method_apply_by_id`). Inline literals
                // have no dedicated spread-aware arm, so every method declines.
                if call.args.iter().any(|a| a.spread.is_some()) {
                    return Ok(Err(args));
                }
                if let ast::Expr::Array(_arr_lit) = member.obj.as_ref() {
                    // Lower the array literal
                    let array_expr = lower_expr(ctx, &member.obj)?;
                    // thisArg routing: the dense `Expr::Array<Method>` fast
                    // paths drop a 2nd positional `thisArg`, so
                    // `[x].map(cb, thisArg)` ran the callback with
                    // `this === undefined`. Route the callback iterators
                    // through the spec-complete `Expr::ArrayLikeMethod`
                    // lowering (which binds the callback `this`) when an
                    // explicit thisArg is supplied with no spread.
                    if matches!(
                        method_name,
                        "map"
                            | "filter"
                            | "forEach"
                            | "find"
                            | "findIndex"
                            | "findLast"
                            | "findLastIndex"
                            | "some"
                            | "every"
                    ) && call.args.len() >= 2
                        && call.args.iter().all(|a| a.spread.is_none())
                    {
                        return Ok(Ok(Expr::ArrayLikeMethod {
                            method: method_name.to_string(),
                            receiver: Box::new(array_expr),
                            args,
                        }));
                    }
                    match method_name {
                        "join" => {
                            // ['a', 'b'].join(separator?) -> string
                            let separator = args.into_iter().next().map(Box::new);
                            return Ok(Ok(Expr::ArrayJoin {
                                array: Box::new(array_expr),
                                separator,
                            }));
                        }
                        "map"
                            if !args.is_empty() => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                return Ok(Ok(Expr::ArrayMap {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                }));
                            }
                        "filter"
                            if !args.is_empty() => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                return Ok(Ok(Expr::ArrayFilter {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                }));
                            }
                        "forEach"
                            if !args.is_empty() => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                return Ok(Ok(Expr::ArrayForEach {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                }));
                            }
                        "find"
                            if !args.is_empty() => {
                                let cb = args.into_iter().next().unwrap();
                                let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                return Ok(Ok(Expr::ArrayFind {
                                    array: Box::new(array_expr),
                                    callback: Box::new(cb),
                                }));
                            }
                        "sort"
                            if !args.is_empty() => {
                                return Ok(Ok(Expr::ArraySort {
                                    array: Box::new(array_expr),
                                    comparator: Box::new(args.into_iter().next().unwrap()),
                                }));
                            }
                        "indexOf"
                            // #2804: carry the optional fromIndex (2nd arg).
                            if !args.is_empty() => {
                                let mut it = args.into_iter();
                                let value = it.next().unwrap();
                                let from_index = it.next().map(Box::new);
                                return Ok(Ok(Expr::ArrayIndexOf {
                                    array: Box::new(array_expr),
                                    value: Box::new(value),
                                    from_index,
                                }));
                            }
                        "includes"
                            if !args.is_empty() => {
                                let mut it = args.into_iter();
                                let value = it.next().unwrap();
                                let from_index = it.next().map(Box::new);
                                return Ok(Ok(Expr::ArrayIncludes {
                                    array: Box::new(array_expr),
                                    value: Box::new(value),
                                    from_index,
                                }));
                            }
                        "slice"
                            if !args.is_empty() => {
                                let mut args_iter = args.into_iter();
                                let start = args_iter.next().unwrap();
                                let end = args_iter.next();
                                return Ok(Ok(Expr::ArraySlice {
                                    array: Box::new(array_expr),
                                    start: Box::new(start),
                                    end: end.map(Box::new),
                                }));
                            }
                        "reduce"
                            if !args.is_empty() => {
                                let mut args_iter = args.into_iter();
                                let callback = args_iter.next().unwrap();
                                let initial = args_iter.next().map(Box::new);
                                return Ok(Ok(Expr::ArrayReduce {
                                    array: Box::new(array_expr),
                                    callback: Box::new(callback),
                                    initial,
                                }));
                            }
                        "flat"
                            // depth-aware calls fall through.
                            if args.is_empty() => {
                                return Ok(Ok(Expr::ArrayFlat {
                                    array: Box::new(array_expr),
                                }));
                            }
                        "reduceRight"
                            if !args.is_empty() => {
                                let mut args_iter = args.into_iter();
                                let callback = args_iter.next().unwrap();
                                let initial = args_iter.next().map(Box::new);
                                return Ok(Ok(Expr::ArrayReduceRight {
                                    array: Box::new(array_expr),
                                    callback: Box::new(callback),
                                    initial,
                                }));
                            }
                        "toReversed" => {
                            return Ok(Ok(Expr::ArrayToReversed {
                                array: Box::new(array_expr),
                            }));
                        }
                        "toSorted" => {
                            let comparator = args.into_iter().next().map(Box::new);
                            return Ok(Ok(Expr::ArrayToSorted {
                                array: Box::new(array_expr),
                                comparator,
                            }));
                        }
                        "toSpliced" => {
                            // #2794: handle omitted args.
                            let arg_count = args.len();
                            let mut args_iter = args.into_iter();
                            let start = args_iter.next().unwrap_or(Expr::Number(0.0));
                            let delete_count = match args_iter.next() {
                                Some(dc) => dc,
                                None if arg_count >= 1 => Expr::Number(f64::INFINITY),
                                None => Expr::Number(0.0),
                            };
                            let items: Vec<Expr> = args_iter.collect();
                            return Ok(Ok(Expr::ArrayToSpliced {
                                array: Box::new(array_expr),
                                start: Box::new(start),
                                delete_count: Box::new(delete_count),
                                items,
                            }));
                        }
                        "with"
                            if args.len() >= 2 => {
                                let mut args_iter = args.into_iter();
                                let index = args_iter.next().unwrap();
                                let value = args_iter.next().unwrap();
                                return Ok(Ok(Expr::ArrayWith {
                                    array: Box::new(array_expr),
                                    index: Box::new(index),
                                    value: Box::new(value),
                                }));
                            }
                        "entries" => {
                            return Ok(Ok(Expr::ArrayEntries(Box::new(array_expr))));
                        }
                        "keys" => {
                            return Ok(Ok(Expr::ArrayKeys(Box::new(array_expr))));
                        }
                        "values" => {
                            return Ok(Ok(Expr::ArrayValues(Box::new(array_expr))));
                        }
                        _ => {} // Fall through for other methods
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
