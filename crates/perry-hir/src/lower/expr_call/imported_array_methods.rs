//! Array methods on imported variables (e.g., `CHAIN_NAMES.join`).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::LoweringContext;

pub(super) fn try_imported_array_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for array methods on imported variables (e.g., import { CHAIN_NAMES } from './module')
    // These don't have local IDs but are ExternFuncRef values
    if let ast::Callee::Expr(expr) = &call.callee {
        if let ast::Expr::Member(member) = expr.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.as_ref();
                if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                    let arr_name = arr_ident.sym.to_string();
                    // A module namespace import (`import * as NS from "..."`) is
                    // NOT an array — its `.map`/`.filter`/`.find`/... are member
                    // functions, e.g. effect's `export const map = core.map`.
                    // Folding `NS.map(x, f)` to `Expr::ArrayMap { array: NS,
                    // callback: x }` dispatched `js_array_map(NS, x)` and
                    // returned `[]` without ever calling the member (#321 —
                    // `Effect.map(...)` never ran). Skip the fold for
                    // namespaces; the generic call path invokes the member
                    // correctly. Named-value imports (`import { CHAIN_NAMES }`)
                    // are not namespaces, so real imported arrays still fold.
                    if ctx.namespace_import_locals.contains(&arr_name) {
                        return Ok(Err(args));
                    }
                    // Check if this is an imported variable (not a local)
                    if ctx.lookup_local(&arr_name).is_none() {
                        if let Some(orig_name) = ctx.lookup_imported_func(&arr_name) {
                            // This is an imported variable - create ExternFuncRef for it
                            let (param_types, return_type) = ctx
                                .lookup_extern_func_types(orig_name)
                                .map(|(p, r)| (p.clone(), r.clone()))
                                .unwrap_or_else(|| (Vec::new(), Type::Any));
                            let extern_ref = Expr::ExternFuncRef {
                                name: orig_name.to_string(),
                                param_types,
                                return_type,
                            };
                            let extern_is_array = matches!(
                                extern_ref,
                                Expr::ExternFuncRef { ref return_type, .. }
                                    if matches!(return_type, Type::Array(_))
                            );
                            match method_name {
                                "join" => {
                                    // Issue #420 (drizzle): `sql.join(arr)` — `sql`
                                    // is imported from drizzle-orm as a tag function
                                    // with a custom `.join` static method. Pre-fix
                                    // this path unconditionally folded to
                                    // ArrayJoin, so `sql.join(valuesSqlList)` was
                                    // dispatched as `js_array_join(sql, list)`
                                    // (treating `sql` as the array, list as the
                                    // separator). Result: empty string back.
                                    //
                                    // Only fold when the imported variable's
                                    // return_type is statically Array. Otherwise
                                    // fall through to generic dispatch which
                                    // respects the imported's own `.join` method.
                                    if matches!(extern_ref, Expr::ExternFuncRef { ref return_type, .. } if matches!(return_type, Type::Array(_)))
                                    {
                                        let separator = args.into_iter().next().map(Box::new);
                                        return Ok(Ok(Expr::ArrayJoin {
                                            array: Box::new(extern_ref),
                                            separator,
                                        }));
                                    }
                                    // Fall through.
                                }
                                "map"
                                    if !args.is_empty() && extern_is_array => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayMap {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "filter"
                                    if !args.is_empty() && extern_is_array => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayFilter {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "forEach"
                                    if !args.is_empty() && extern_is_array => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayForEach {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "find"
                                    if !args.is_empty() && extern_is_array => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayFind {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "sort"
                                    // Like `join` above: only fold when the
                                    // imported binding is statically Array-typed.
                                    // semver re-exports `sort = (list) =>
                                    // list.sort(cmp)` and the driver calls
                                    // `semver.sort(list)`; `semver` is an imported
                                    // module-exports object (return_type Any), so
                                    // folding to `Expr::ArraySort { array: semver,
                                    // comparator: list }` mis-routed the single
                                    // `list` arg into the comparator slot →
                                    // "comparison function must be either a
                                    // function or undefined". Fall through to the
                                    // generic call path, which invokes the imported
                                    // `sort` function correctly.
                                    if !args.is_empty()
                                        && matches!(
                                            extern_ref,
                                            Expr::ExternFuncRef { ref return_type, .. }
                                                if matches!(return_type, Type::Array(_))
                                        )
                                    => {
                                        return Ok(Ok(Expr::ArraySort {
                                            array: Box::new(extern_ref),
                                            comparator: Box::new(args.into_iter().next().unwrap()),
                                        }));
                                    }
                                "indexOf"
                                    // #2804: carry the optional fromIndex (2nd arg).
                                    if !args.is_empty() && extern_is_array => {
                                        let mut it = args.into_iter();
                                        let value = it.next().unwrap();
                                        let from_index = it.next().map(Box::new);
                                        return Ok(Ok(Expr::ArrayIndexOf {
                                            array: Box::new(extern_ref),
                                            value: Box::new(value),
                                            from_index,
                                        }));
                                    }
                                "includes"
                                    if !args.is_empty() && extern_is_array => {
                                        let mut it = args.into_iter();
                                        let value = it.next().unwrap();
                                        let from_index = it.next().map(Box::new);
                                        return Ok(Ok(Expr::ArrayIncludes {
                                            array: Box::new(extern_ref),
                                            value: Box::new(value),
                                            from_index,
                                        }));
                                    }
                                "slice"
                                    if !args.is_empty() && extern_is_array => {
                                        let mut args_iter = args.into_iter();
                                        let start = args_iter.next().unwrap();
                                        let end = args_iter.next();
                                        return Ok(Ok(Expr::ArraySlice {
                                            array: Box::new(extern_ref),
                                            start: Box::new(start),
                                            end: end.map(Box::new),
                                        }));
                                    }
                                "reduce"
                                    if !args.is_empty() && extern_is_array => {
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Ok(Expr::ArrayReduce {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(callback),
                                            initial,
                                        }));
                                    }
                                "flat"
                                    // depth-aware calls fall through.
                                    if args.is_empty() && extern_is_array => {
                                        return Ok(Ok(Expr::ArrayFlat {
                                            array: Box::new(extern_ref),
                                        }));
                                    }
                                "reduceRight"
                                    if !args.is_empty() && extern_is_array => {
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Ok(Expr::ArrayReduceRight {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(callback),
                                            initial,
                                        }));
                                    }
                                "toReversed" if extern_is_array => {
                                    return Ok(Ok(Expr::ArrayToReversed {
                                        array: Box::new(extern_ref),
                                    }));
                                }
                                "toSorted" if extern_is_array => {
                                    let comparator = args.into_iter().next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayToSorted {
                                        array: Box::new(extern_ref),
                                        comparator,
                                    }));
                                }
                                "toSpliced" if extern_is_array => {
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
                                        array: Box::new(extern_ref),
                                        start: Box::new(start),
                                        delete_count: Box::new(delete_count),
                                        items,
                                    }));
                                }
                                "with"
                                    if args.len() >= 2 && extern_is_array => {
                                        let mut args_iter = args.into_iter();
                                        let index = args_iter.next().unwrap();
                                        let value = args_iter.next().unwrap();
                                        return Ok(Ok(Expr::ArrayWith {
                                            array: Box::new(extern_ref),
                                            index: Box::new(index),
                                            value: Box::new(value),
                                        }));
                                    }
                                "entries" if extern_is_array => {
                                    return Ok(Ok(Expr::ArrayEntries(Box::new(extern_ref))));
                                }
                                "keys" if extern_is_array => {
                                    return Ok(Ok(Expr::ArrayKeys(Box::new(extern_ref))));
                                }
                                "values" if extern_is_array => {
                                    return Ok(Ok(Expr::ArrayValues(Box::new(extern_ref))));
                                }
                                _ => {} // Fall through for other methods
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Err(args))
}
