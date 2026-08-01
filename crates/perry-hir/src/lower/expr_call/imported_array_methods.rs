//! Array methods on imported variables (e.g., `CHAIN_NAMES.join`).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.
//!
//! # Invariant (#7154)
//!
//! An imported binding's `.map`/`.filter`/`.slice`/… is folded to the
//! corresponding `Expr::Array*` builtin **only when the binding is statically
//! typed as an array**. Method NAME alone is not evidence: a module is free to
//! export an object (or a function with statics) whose members happen to be
//! called `map`, `filter`, `keys`, `slice`, … and folding those rewrites a
//! call to a completely different function — silently dropping the arguments
//! past the first and passing argument #1 into the Array builtin's callback /
//! separator / index slot.
//!
//! Two arms already learned this the hard way and carried a local guard
//! (`join`, #420 drizzle's `sql.join(list)`; `sort`, semver's
//! `sort(list)`). The remaining ~18 arms did not, so e.g. zod's
//! `z.map(keyType, valueType)` — `z` is a named import of a module namespace
//! object, `map` is `export function map(k, v)` — lowered to
//! `Expr::ArrayMap { array: z, callback: keyType }`, dropped `valueType`, and
//! threw `TypeError: object is not a function` from
//! `js_validate_array_callback`. The guard is now applied once, up front, for
//! every method name.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

use super::super::LoweringContext;

/// Is this imported binding statically known to hold an array?
///
/// The only positive evidence available at this point is the declared type
/// recorded for the imported binding (`lookup_extern_func_types`'s return
/// type). `Type::Any` — the fallback used when nothing is known — is **not**
/// evidence and must decline, which is what keeps a non-array export from
/// being rewritten into an Array builtin.
fn imported_binding_is_array(extern_ref: &Expr) -> bool {
    let Expr::ExternFuncRef { return_type, .. } = extern_ref else {
        return false;
    };
    match return_type {
        Type::Array(_) | Type::Tuple(_) => true,
        Type::Generic { base, .. } => base == "Array" || base == "ReadonlyArray",
        _ => false,
    }
}

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
                // NOTE (#6718): a spread method call on an IMPORTED array
                // (`CHAIN_NAMES.map(...[fn])`, `CHAIN_NAMES.slice(...[1,3])`) is
                // intentionally NOT declined here — unlike the local/inline/
                // array-only fast paths, which decline any spread to the generic
                // `Expr::CallSpread` tail. The imported binding lowers to an
                // `ExternFuncRef` receiver, which that tail's member-callee arm
                // skips (it may be an imported *function*), so routing there would
                // fail to dispatch the native method. This niche receiver stays at
                // its pre-existing behavior rather than trading one failure for
                // another; the #6718 repro covers inline literals + local
                // receivers + queueMicrotask.
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
                            // #7154: no static array evidence → the binding may
                            // be any exported value that merely has a member of
                            // this name. Decline to the generic call path, which
                            // invokes the member itself with every argument.
                            if !imported_binding_is_array(&extern_ref) {
                                return Ok(Err(args));
                            }
                            match method_name {
                                "join" => {
                                    // Issue #420 (drizzle): `sql.join(arr)` — `sql`
                                    // is imported from drizzle-orm as a tag function
                                    // with a custom `.join` static method. Pre-fix
                                    // this path unconditionally folded to
                                    // ArrayJoin, so `sql.join(valuesSqlList)` was
                                    // dispatched as `js_array_join(sql, list)`
                                    // (treating `sql` as the array, list as the
                                    // separator). Result: empty string back. The
                                    // static-array gate above now covers this.
                                    let separator = args.into_iter().next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayJoin {
                                        array: Box::new(extern_ref),
                                        separator,
                                    }));
                                }
                                "map"
                                    if !args.is_empty() => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayMap {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "filter"
                                    if !args.is_empty() => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayFilter {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "forEach"
                                    if !args.is_empty() => {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        return Ok(Ok(Expr::ArrayForEach {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(cb),
                                        }));
                                    }
                                "find"
                                    if !args.is_empty() => {
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
                                    // function or undefined". The static-array gate
                                    // above now covers this; the arg check stays
                                    // because a 0-arg `sort()` has no comparator.
                                    if !args.is_empty() => {
                                        return Ok(Ok(Expr::ArraySort {
                                            array: Box::new(extern_ref),
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
                                            array: Box::new(extern_ref),
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
                                            array: Box::new(extern_ref),
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
                                            array: Box::new(extern_ref),
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
                                            array: Box::new(extern_ref),
                                            callback: Box::new(callback),
                                            initial,
                                        }));
                                    }
                                "flat"
                                    // depth-aware calls fall through.
                                    if args.is_empty() => {
                                        return Ok(Ok(Expr::ArrayFlat {
                                            array: Box::new(extern_ref),
                                        }));
                                    }
                                "reduceRight"
                                    if !args.is_empty() => {
                                        let mut args_iter = args.into_iter();
                                        let callback = args_iter.next().unwrap();
                                        let initial = args_iter.next().map(Box::new);
                                        return Ok(Ok(Expr::ArrayReduceRight {
                                            array: Box::new(extern_ref),
                                            callback: Box::new(callback),
                                            initial,
                                        }));
                                    }
                                "toReversed" => {
                                    return Ok(Ok(Expr::ArrayToReversed {
                                        array: Box::new(extern_ref),
                                    }));
                                }
                                "toSorted" => {
                                    let comparator = args.into_iter().next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayToSorted {
                                        array: Box::new(extern_ref),
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
                                        array: Box::new(extern_ref),
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
                                            array: Box::new(extern_ref),
                                            index: Box::new(index),
                                            value: Box::new(value),
                                        }));
                                    }
                                "entries" => {
                                    return Ok(Ok(Expr::ArrayEntries(Box::new(extern_ref))));
                                }
                                "keys" => {
                                    return Ok(Ok(Expr::ArrayKeys(Box::new(extern_ref))));
                                }
                                "values" => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionType, ObjectType};

    fn extern_ref(return_type: Type) -> Expr {
        Expr::ExternFuncRef {
            name: "z".to_string(),
            param_types: Vec::new(),
            return_type,
        }
    }

    #[test]
    fn static_array_evidence_admits_the_fold() {
        assert!(imported_binding_is_array(&extern_ref(Type::Array(
            Box::new(Type::Number)
        ))));
        assert!(imported_binding_is_array(&extern_ref(Type::Tuple(vec![
            Type::Number,
            Type::String
        ]))));
        assert!(imported_binding_is_array(&extern_ref(Type::Generic {
            base: "Array".to_string(),
            type_args: vec![Type::Number],
        })));
        assert!(imported_binding_is_array(&extern_ref(Type::Generic {
            base: "ReadonlyArray".to_string(),
            type_args: vec![Type::Number],
        })));
    }

    #[test]
    fn absent_or_non_array_evidence_declines_the_fold() {
        // `Type::Any` is the fallback used whenever nothing is known about the
        // imported binding — the zod `z.map(keyType, valueType)` case. It must
        // NOT be read as "probably an array".
        assert!(!imported_binding_is_array(&extern_ref(Type::Any)));
        assert!(!imported_binding_is_array(&extern_ref(Type::Unknown)));
        assert!(!imported_binding_is_array(&extern_ref(Type::Object(
            ObjectType::default()
        ))));
        assert!(!imported_binding_is_array(&extern_ref(Type::Function(
            FunctionType {
                params: Vec::new(),
                return_type: Box::new(Type::Any),
                is_async: false,
                is_generator: false,
            }
        ))));
        assert!(!imported_binding_is_array(&extern_ref(Type::Generic {
            base: "Map".to_string(),
            type_args: vec![Type::String, Type::Number],
        })));
        // A non-`ExternFuncRef` receiver never reaches the fold either.
        assert!(!imported_binding_is_array(&Expr::Number(1.0)));
    }
}
