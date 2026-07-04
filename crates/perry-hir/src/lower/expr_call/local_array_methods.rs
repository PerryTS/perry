//! Array method calls on local-variable receivers (arr.push, arr.pop, etc.).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::Result;
use perry_types::Type;
use swc_ecma_ast as ast;

use super::url_search_params::build_url_search_params_method_call;
use crate::ir::*;

use super::super::{lower_expr, LoweringContext};

/// True when `recv_ty` is a statically-known class/namespace instance type
/// (a `Named` or `Generic` type that is not itself an array). Used to gate
/// array-method folds (`.sort`/`.map`/…) so a user/library method that merely
/// shares a builtin Array name (e.g. semver's `semver.sort(list)`) is not
/// rewritten into the corresponding `Expr::Array*` fast path. TypedArray
/// `Named` types are deliberately treated as arrays (not class instances).
fn receiver_is_class_instance(recv_ty: Option<&Type>) -> bool {
    let is_typed_array = |n: &str| {
        matches!(
            n,
            "Int8Array"
                | "Int16Array"
                | "Int32Array"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Uint16Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
        )
    };
    match recv_ty {
        Some(Type::Named(n)) => !is_typed_array(n),
        Some(Type::Generic { base, .. }) => base != "Array",
        _ => false,
    }
}

pub(super) fn try_local_array_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    expr: &ast::Expr,
    mut args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    if let ast::Expr::Member(member) = expr {
        // Check for array method calls (arr.push, arr.pop, etc.)
        // These are called on local variables, not global modules
        // IMPORTANT: Only apply to actual Array types, not String types
        if let ast::MemberProp::Ident(method_ident) = &member.prop {
            let method_name = method_ident.sym.as_ref();
            if let ast::Expr::Ident(arr_ident) = member.obj.as_ref() {
                let arr_name = arr_ident.sym.to_string();
                // #5196: a Proxy-wrapped array routes ALL its method calls
                // through the proxy member-call path (`ProxyGet` +
                // `js_native_call_method`), so the method's `this` binds to the
                // proxy and element reads fire its `get` trap. Folding to the
                // dense `Expr::Array*` fast paths below would dereference the
                // proxy id as a real `ArrayHeader` and SIGSEGV. `arr.map`/
                // `.filter`/`.find` already escaped via the `is_class_instance`
                // gate (a proxy local is typed `Named("Proxy")`), but
                // `reduce`/`forEach`/`join`/`sort`/`splice`/… did not — guard
                // them all here, uniformly, by falling through.
                if ctx.proxy_locals.contains(&arr_name) {
                    return Ok(Err(args));
                }
                // Check that this is NOT a String type (Array, Set, Map are all OK)
                // When type is unknown, only enter array block for array-only methods
                // (push, pop, etc.), NOT for methods shared with strings (indexOf,
                // includes, split) — those are handled by the general dispatch which
                // checks is_string at codegen time.
                let type_info = ctx.lookup_local_type(&arr_name);
                // `Union<String, Void>` (e.g. `JSON.stringify` return type) is
                // a possible-string — must NOT be treated as definitely not-a-
                // string, otherwise `.indexOf`/`.includes` get routed through
                // ArrayIndexOf/ArrayIncludes and return -1/false on a real
                // string value.
                let is_union_with_string = matches!(
                    type_info,
                    Some(Type::Union(variants)) if variants.iter().any(|v| matches!(v, Type::String))
                );
                // A boxed `String` wrapper (`new String("x")`, type `Named("String")`)
                // is NOT an array: the ambiguous methods shared with Array
                // (`indexOf`/`includes`/`slice`/`lastIndexOf`) must route to the
                // string dispatch (which `ToString`-coerces the wrapper), not to
                // `ArrayIndexOf`/`ArrayIncludes` (which read it as an array and
                // return -1/false). `search`/`match`/`split` already bypass this
                // file because they aren't Array methods.
                let is_boxed_string_wrapper =
                    matches!(type_info, Some(Type::Named(n)) if n == "String");
                let is_known_string = type_info
                    .map(|ty| matches!(ty, Type::String))
                    .unwrap_or(false)
                    || is_union_with_string
                    || is_boxed_string_wrapper;
                // A user-defined class instance is NOT an array — must skip the array
                // fast path so user-defined methods like Stack<T>.push() are dispatched
                // to the class method, not runtime js_array_push. Map/Set/Promise are
                // handled by explicit checks within the array block below.
                let builtin_generic_bases = ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                // Imported classes don't show up in `lookup_class`; treat any
                // uppercase imported identifier as a candidate class so the
                // array fast-path doesn't swallow `coll.find(filter)` etc.
                let is_imported_class_name = |n: &str| -> bool {
                    if let Some(c) = n.chars().next() {
                        if c.is_uppercase() && ctx.lookup_imported_func(n).is_some() {
                            return true;
                        }
                    }
                    false
                };
                let is_user_class_instance = match type_info {
                    // A class instance OR an interface-typed value is the
                    // receiver's own object — its method must be dispatched, not
                    // the array fast path. Interfaces aren't classes (so
                    // `lookup_class` misses them); without `is_interface_type`,
                    // an interface-typed receiver with e.g. an own `push` folded
                    // to `Expr::ArrayPush`, read the object header as an
                    // ArrayHeader, and silently dropped the call (follow-up to
                    // #5139, which fixed only `any`-typed receivers).
                    Some(Type::Named(name)) => {
                        ctx.lookup_class(name).is_some()
                            || ctx.is_interface_type(name)
                            || is_imported_class_name(name)
                    }
                    Some(Type::Generic { base, .. }) => {
                        !builtin_generic_bases.contains(&base.as_str())
                            && (ctx.lookup_class(base).is_some() || is_imported_class_name(base))
                    }
                    _ => false,
                };
                // When the receiver type is Any and the method name is one
                // commonly defined on user classes too (e.g. mongo's
                // `Collection.find(filter)`), skip the array fast-path so the
                // dispatch falls through to class-method resolution. Without this
                // guard, the lowering blindly emits `Expr::ArrayFind` and the
                // call resolves to `js_array_find` at codegen time, returning 0.
                let is_class_overlapping_method = matches!(
                    method_name,
                    "find"
                        | "findIndex"
                        | "findLast"
                        | "findLastIndex"
                        | "map"
                        | "filter"
                        | "some"
                        | "every"
                        | "forEach"
                        | "reduce"
                        | "reduceRight"
                        | "join"
                        // wall 49: the mutating array methods are ALSO commonly
                        // user-class methods (Stack.push, Queue.shift, Next.js
                        // `DefaultRouteMatcherManager.push`). On an unknown (`Any`)
                        // receiver the inline array fast path reads the instance's
                        // ObjectHeader as an ArrayHeader and corrupts it; route
                        // through dynamic dispatch instead, which handles both real
                        // arrays and class instances correctly. Typed arrays
                        // (`Type::Array`) are unaffected — `is_unknown_recv` is
                        // false for them, so they keep the fast path.
                        | "push"
                        | "pop"
                        | "shift"
                        | "unshift"
                );
                let is_unknown_recv =
                    matches!(type_info, None | Some(Type::Any) | Some(Type::Unknown));
                // #5139: Array-mutator names that a plain object can also own as a
                // closure-valued property. The runtime's `js_native_call_method`
                // dispatches all of these correctly on either a real array or a
                // plain object (see `try_object_arraylike_mutator` + the generic
                // own-field scan), so for an `any`-typed receiver we defer to it
                // rather than committing to the array-only fast path. Mirrors the
                // method set special-cased in
                // `array::try_object_arraylike_mutator`.
                let is_arraylike_mutator_method = matches!(
                    method_name,
                    "push" | "pop" | "shift" | "unshift" | "reverse" | "splice" | "sort" | "concat"
                );
                let is_known_not_string = type_info
                    .map(|ty| !matches!(ty, Type::String | Type::Any | Type::Unknown))
                    .unwrap_or(false)
                    && !is_union_with_string;
                // Object type literals (e.g., { push: (v: number) => void; ... })
                // are NOT arrays — they are plain objects with closure-valued
                // properties and must NOT enter the array fast path.
                let is_object_type = matches!(type_info, Some(Type::Object(_)));
                // `Uint8Array`/`Buffer` instances must NOT enter the generic
                // array fast path. They have a distinct runtime representation
                // (raw `BufferHeader`, no f64 elements) and a different method
                // family (`readUInt8`, `swap16`, byte-level `indexOf` matching
                // string/buffer needles, etc.). The runtime's
                // `dispatch_buffer_method` handles all of these via the
                // universal `js_native_call_method` fallback path.
                let is_buffer_type = matches!(
                    type_info,
                    Some(Type::Named(n))
                        if n == "Uint8Array" || n == "Buffer" || n == "Uint8ClampedArray"
                );
                let is_node_stream_readable_type = matches!(
                    type_info,
                    Some(Type::Named(n))
                        if matches!(
                            n.as_str(),
                            "Readable" | "Duplex" | "Transform" | "PassThrough"
                        )
                );
                let is_ambiguous_method = matches!(
                    method_name,
                    "indexOf" | "includes" | "slice" | "lastIndexOf"
                );
                let is_not_string = if is_known_string {
                    false // definitely a string, skip array block
                } else if is_user_class_instance {
                    false // user class — must dispatch to class method, skip array fast-path
                } else if is_object_type {
                    false // object type literal — dispatch via method call, not array ops
                } else if is_buffer_type {
                    false // Buffer/Uint8Array — runtime dispatch handles byte-level methods
                } else if is_node_stream_readable_type {
                    false // Node streams expose iterator helpers with Array-like names
                } else if is_known_not_string {
                    true // definitely not a string, enter array block
                } else if is_ambiguous_method {
                    false // type unknown + ambiguous method, skip array block (fall through to general dispatch)
                } else if is_unknown_recv && is_class_overlapping_method {
                    false // type unknown + method commonly defined on user classes — fall through
                } else if is_unknown_recv && is_arraylike_mutator_method {
                    // #5139: type unknown + an Array-mutator name that a plain
                    // object can legitimately own (`{ push(c) {…} }` passed as
                    // `any` — e.g. react-dom/server's SSR `destination`). Eagerly
                    // emitting `Expr::ArrayPush`/etc. reads the object's header as
                    // an `ArrayHeader` and corrupts it (push returns a bogus length
                    // and never runs the user method). Fall through to the runtime
                    // `js_native_call_method` dispatch, which inspects the actual
                    // receiver shape: real array → dense `js_array_push_f64`; plain
                    // object with an own callable of this name → that method (this
                    // case routes via `try_object_arraylike_mutator`, whose
                    // own-user-method gate returns `None`, then the generic
                    // own-field scan invokes the closure with `this` = receiver).
                    false
                } else {
                    true // type unknown + array-only method (push, pop, etc.), enter array block
                };
                // Helper: if the callback arg is a bare Boolean/Number/String identifier,
                // desugar to a synthetic closure: x => Boolean(x) / Number(x) / String(x).
                // This is needed because .filter(Boolean) etc. expect a closure pointer at
                // runtime but built-in constructors aren't first-class closure objects.
                if is_not_string {
                    if let Some(array_id) = ctx.lookup_local(&arr_name) {
                        // thisArg routing: the dense `Expr::Array<Method>` fast
                        // paths drop a 2nd positional `thisArg`, so
                        // `arr.every(cb, thisArg)` ran the callback with
                        // `this === undefined`. Route the callback iterators
                        // through the spec-complete `Expr::ArrayLikeMethod`
                        // lowering (which binds the callback `this`) when an
                        // explicit thisArg is supplied with no spread.
                        // Map/Set/URLSearchParams keep their own forEach contract
                        // (thisArg binding via `js_{map,set}_foreach`); folding
                        // `set.forEach(cb, thisArg)` into the array-like path ran
                        // the callback against an array view → zero iterations
                        // (test262 Set/Map forEach this-arg-explicit). The 1-arg
                        // `match` below already excludes them; mirror that here.
                        let recv_is_non_array_collection = {
                            let is_nac = |ty: &Type| {
                                matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set")
                                    || matches!(ty, Type::Named(n) if n == "URLSearchParams")
                            };
                            match ctx.lookup_local_type(&arr_name) {
                                Some(ty) if is_nac(ty) => true,
                                Some(Type::Union(variants)) => variants.iter().any(is_nac),
                                _ => false,
                            }
                        };
                        if !recv_is_non_array_collection
                            && matches!(
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
                            )
                            && call.args.len() >= 2
                            && call.args.iter().all(|a| a.spread.is_none())
                        {
                            return Ok(Ok(Expr::ArrayLikeMethod {
                                method: method_name.to_string(),
                                receiver: Box::new(Expr::LocalGet(array_id)),
                                args,
                            }));
                        }
                        match method_name {
                            "push" => {
                                if args.is_empty() {
                                    // `arr.push()` with no items still performs
                                    // `Set(O,"length",…,true)` (ECMA-262
                                    // §23.1.3.21 step 6), so a frozen array or one
                                    // whose `length` is non-writable must throw a
                                    // TypeError — it is NOT a pure `length` read
                                    // (test262 push/set-length-zero-array-is-frozen
                                    // and set-length-zero-array-length-is-non-writable).
                                    // Route through the native push dispatch, which
                                    // emits `js_array_push_guard`.
                                    return Ok(Ok(Expr::NativeMethodCall {
                                        module: "array".to_string(),
                                        class_name: None,
                                        object: Some(Box::new(Expr::LocalGet(array_id))),
                                        method: "push".to_string(),
                                        args: vec![],
                                    }));
                                }
                                // Check if any argument has spread operator —
                                // when present, route through the spread path.
                                // Multi-arg push without spread is desugared to a
                                // Sequence of ArrayPush expressions (one per arg);
                                // JS spec returns the final array length, which is
                                // exactly what the last ArrayPush returns.
                                let any_spread = call.args.iter().any(|a| a.spread.is_some());
                                if any_spread {
                                    if args.len() == 1 && call.args[0].spread.is_some() {
                                        return Ok(Ok(Expr::ArrayPushSpread {
                                            array_id,
                                            source: Box::new(args.into_iter().next().unwrap()),
                                        }));
                                    }
                                    let mut stmts: Vec<Expr> = Vec::with_capacity(args.len());
                                    for (ast_arg, arg) in call.args.iter().zip(args) {
                                        if ast_arg.spread.is_some() {
                                            stmts.push(Expr::ArrayPushSpread {
                                                array_id,
                                                source: Box::new(arg),
                                            });
                                        } else {
                                            stmts.push(Expr::ArrayPush {
                                                array_id,
                                                value: Box::new(arg),
                                            });
                                        }
                                    }
                                    return Ok(Ok(Expr::Sequence(stmts)));
                                } else {
                                    if args.len() == 1 {
                                        return Ok(Ok(Expr::ArrayPush {
                                            array_id,
                                            value: Box::new(args.into_iter().next().unwrap()),
                                        }));
                                    }
                                    let mut stmts: Vec<Expr> = Vec::with_capacity(args.len());
                                    for a in args.into_iter() {
                                        stmts.push(Expr::ArrayPush {
                                            array_id,
                                            value: Box::new(a),
                                        });
                                    }
                                    return Ok(Ok(Expr::Sequence(stmts)));
                                }
                            }
                            "pop" => {
                                return Ok(Ok(Expr::ArrayPop(array_id)));
                            }
                            "shift" => {
                                return Ok(Ok(Expr::ArrayShift(array_id)));
                            }
                            "unshift"
                                // #2814: the single-value fast path only handles
                                // exactly one argument. Zero-arg and multi-arg
                                // calls fall through to generic dispatch, which
                                // routes to the variadic runtime helper.
                                if args.len() == 1 => {
                                    return Ok(Ok(Expr::ArrayUnshift {
                                        array_id,
                                        value: Box::new(args.into_iter().next().unwrap()),
                                    }));
                                }
                            "indexOf"
                                // #2804: carry the optional fromIndex (2nd arg).
                                if !args.is_empty() => {
                                    let mut it = args.into_iter();
                                    let value = it.next().unwrap();
                                    let from_index = it.next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayIndexOf {
                                        array: Box::new(Expr::LocalGet(array_id)),
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
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        value: Box::new(value),
                                        from_index,
                                    }));
                                }
                            // arr.lastIndexOf(value, fromIndex?) — route to the
                            // array runtime fn. Without this, a known-not-string
                            // / typed-array local fell through to the *string*
                            // lastIndexOf (#2457): `new Int32Array(...).lastIndexOf`
                            // threw "(number).lastIndexOf is not a function".
                            "lastIndexOf"
                                if !args.is_empty() => {
                                    let mut it = args.into_iter();
                                    let value = it.next().unwrap();
                                    let from_index = it.next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayLastIndexOf {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        value: Box::new(value),
                                        from_index,
                                    }));
                                }
                            "slice" => {
                                // arr.slice(start, end?) - returns new array
                                // Only convert to ArraySlice if we KNOW it's an Array type
                                // (Type::Any could be a string, which has its own .slice() method).
                                // TypedArray receivers (#3148) also route here so
                                // `int32arr.slice(1,3)` returns a same-kind TypedArray
                                // (js_array_slice delegates via lookup_typed_array_kind).
                                let is_definitely_array = ctx
                                    .lookup_local_type(&arr_name)
                                    .map(|ty| {
                                        matches!(ty, Type::Array(_))
                                            || matches!(ty, Type::Named(n) if matches!(
                                                n.as_str(),
                                                "Int8Array" | "Int16Array" | "Int32Array"
                                                | "Uint16Array" | "Uint32Array"
                                                | "Float16Array" | "Float32Array" | "Float64Array"
                                                | "BigInt64Array" | "BigUint64Array"
                                            ))
                                    })
                                    .unwrap_or(false);
                                if is_definitely_array && !args.is_empty() {
                                    let mut args_iter = args.into_iter();
                                    let start = args_iter.next().unwrap();
                                    let end = args_iter.next();
                                    return Ok(Ok(Expr::ArraySlice {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        start: Box::new(start),
                                        end: end.map(Box::new),
                                    }));
                                }
                                // Fall through to normal Call handling for strings or unknown types
                            }
                            "splice" => {
                                // arr.splice(start, deleteCount?, ...items) - returns deleted elements
                                let has_start = !args.is_empty();
                                let mut args_iter = args.into_iter();
                                let start = args_iter.next().unwrap_or(Expr::Number(0.0));
                                let delete_count = if has_start {
                                    args_iter.next().map(Box::new)
                                } else {
                                    Some(Box::new(Expr::Number(0.0)))
                                };
                                let items: Vec<Expr> = args_iter.collect();
                                return Ok(Ok(Expr::ArraySplice {
                                    array_id,
                                    start: Box::new(start),
                                    delete_count,
                                    items,
                                }));
                            }
                            "forEach" => {
                                // Check if the receiver is a Map or Set - if so, don't use ArrayForEach.
                                // Issue #542/#543: also reject `Map | undefined` / `Set | undefined`
                                // so the same array/Map mismatch on for-of doesn't recur for
                                // forEach calls on optional-Map parameters.
                                // URLSearchParams also has its own forEach contract — the
                                // callback receives `(value, key, this)` (strings) not the
                                // `(item, index)` Array.forEach signature; folding to
                                // ArrayForEach here would pass `(NaN, 0)` to the closure.
                                let recv_ty = ctx.lookup_local_type(&arr_name);
                                let is_non_array_collection = |ty: &Type| -> bool {
                                    matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set")
                                        || matches!(ty, Type::Named(n) if n == "URLSearchParams")
                                };
                                let is_map_or_set = match recv_ty {
                                    Some(ty) if is_non_array_collection(ty) => true,
                                    Some(Type::Union(variants)) => {
                                        variants.iter().any(is_non_array_collection)
                                    }
                                    _ => false,
                                };
                                if !is_map_or_set && !args.is_empty() {
                                    let cb = args.into_iter().next().unwrap();
                                    let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                    return Ok(Ok(Expr::ArrayForEach {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        callback: Box::new(cb),
                                    }));
                                }
                            }
                            "map" | "filter" | "find" | "findIndex" | "findLast"
                            | "findLastIndex" | "some" | "every" | "at" => {
                                // Skip the array-method fast path when the receiver
                                // is a known class instance (e.g. mongo `Collection.find`).
                                // Without this guard, `coll.find(filter)` lowers to
                                // `Expr::ArrayFind` and dispatches to `js_array_find`,
                                // which silently returns 0 on a class receiver.
                                let recv_ty = ctx.lookup_local_type(&arr_name);
                                // TypedArray types are Named but must NOT be treated as
                                // class instances — they need the array-method fast path
                                // so `.at()` / `.findLast()` emit the right HIR variants.
                                let is_typed_array = recv_ty
                                    .as_ref()
                                    .map(|ty| {
                                        matches!(ty, Type::Named(n) if matches!(
                                            n.as_str(),
                                            "Int8Array" | "Int16Array" | "Int32Array"
                                            | "Uint8Array" | "Uint8ClampedArray"
                                            | "Uint16Array" | "Uint32Array"
                                            | "Float16Array" | "Float32Array" | "Float64Array"
                                            | "BigInt64Array" | "BigUint64Array"
                                        ))
                                    })
                                    .unwrap_or(false);
                                let is_class_instance = !is_typed_array
                                    && recv_ty
                                        .as_ref()
                                        .map(|ty| {
                                            matches!(ty, Type::Named(_) | Type::Generic { .. })
                                                && !matches!(ty, Type::Array(_))
                                        })
                                        .unwrap_or(false);
                                // Issue #514: gate `.at()` ArrayAt
                                // emission on a statically-known
                                // array type. `at` is shared between
                                // String.prototype and Array.prototype,
                                // so for `(s: any).at(-1)` codegen
                                // can't tell which one the user means.
                                // Pre-fix the HIR optimistically
                                // lowered to `Expr::ArrayAt` →
                                // `js_array_at`, which interprets
                                // the NaN-boxed *StringHeader as an
                                // *ArrayHeader and returns garbage.
                                // Now: only emit ArrayAt for proven
                                // arrays / typed-arrays; otherwise
                                // fall through to the generic method
                                // dispatch which lands in the runtime
                                // tower's tag-aware string arm.
                                let recv_is_array =
                                    is_typed_array || matches!(recv_ty, Some(Type::Array(_)));
                                if !is_class_instance {
                                    if method_name == "at" && recv_is_array {
                                        if !args.is_empty() {
                                            return Ok(Ok(Expr::ArrayAt {
                                                array: Box::new(Expr::LocalGet(array_id)),
                                                index: Box::new(args.into_iter().next().unwrap()),
                                            }));
                                        }
                                    } else if method_name != "at" && !args.is_empty() {
                                        let cb = args.into_iter().next().unwrap();
                                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                        let array = Box::new(Expr::LocalGet(array_id));
                                        let callback = Box::new(cb);
                                        return Ok(Ok(match method_name {
                                            "map" => Expr::ArrayMap { array, callback },
                                            "filter" => Expr::ArrayFilter { array, callback },
                                            "find" => Expr::ArrayFind { array, callback },
                                            "findIndex" => Expr::ArrayFindIndex { array, callback },
                                            "findLast" => Expr::ArrayFindLast { array, callback },
                                            "findLastIndex" => {
                                                Expr::ArrayFindLastIndex { array, callback }
                                            }
                                            "some" => Expr::ArraySome { array, callback },
                                            "every" => Expr::ArrayEvery { array, callback },
                                            _ => unreachable!(),
                                        }));
                                    }
                                }
                            }
                            "flatMap"
                                if !args.is_empty() => {
                                    let cb = args.into_iter().next().unwrap();
                                    let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                                    return Ok(Ok(Expr::ArrayFlatMap {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        callback: Box::new(cb),
                                    }));
                                }
                            "sort"
                                // semver `module.exports.sort = (list) => …` is
                                // re-exported as a plain function and called as
                                // `semver.sort(list)`. The receiver there is a
                                // class/namespace instance, NOT an array, so
                                // folding to `Expr::ArraySort` mis-routed the
                                // single `list` argument into the comparator slot
                                // → "comparison function must be either a function
                                // or undefined". Only fold when the receiver is
                                // not a statically-known class instance (mirrors
                                // the `map`/`filter`/`with` guards).
                                if !args.is_empty()
                                    && !receiver_is_class_instance(ctx.lookup_local_type(&arr_name))
                                => {
                                    return Ok(Ok(Expr::ArraySort {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        comparator: Box::new(args.into_iter().next().unwrap()),
                                    }));
                                }
                            "reduce"
                                if !args.is_empty() => {
                                    let mut args_iter = args.into_iter();
                                    let callback = args_iter.next().unwrap();
                                    let initial = args_iter.next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayReduce {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        callback: Box::new(callback),
                                        initial,
                                    }));
                                }
                            "join" => {
                                // arr.join(separator?) -> string
                                let separator = args.into_iter().next().map(Box::new);
                                return Ok(Ok(Expr::ArrayJoin {
                                    array: Box::new(Expr::LocalGet(array_id)),
                                    separator,
                                }));
                            }
                            "flat"
                                // arr.flat() folds to depth=1 fast path;
                                // arr.flat(depth) falls through so the
                                // depth arg can reach the codegen
                                // `lower_array_method.rs::flat` arm and
                                // route to `js_array_flat_depth`.
                                if args.is_empty() => {
                                    return Ok(Ok(Expr::ArrayFlat {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                    }));
                                }
                            "reduceRight"
                                if !args.is_empty() => {
                                    let mut args_iter = args.into_iter();
                                    let callback = args_iter.next().unwrap();
                                    let initial = args_iter.next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayReduceRight {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        callback: Box::new(callback),
                                        initial,
                                    }));
                                }
                            "toReversed" => {
                                return Ok(Ok(Expr::ArrayToReversed {
                                    array: Box::new(Expr::LocalGet(array_id)),
                                }));
                            }
                            "toSorted" => {
                                let comparator = args.into_iter().next().map(Box::new);
                                return Ok(Ok(Expr::ArrayToSorted {
                                    array: Box::new(Expr::LocalGet(array_id)),
                                    comparator,
                                }));
                            }
                            "toSpliced" => {
                                // #2794: handle omitted args (0 -> copy, 1 ->
                                // delete through end via +Infinity deleteCount).
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
                                    array: Box::new(Expr::LocalGet(array_id)),
                                    start: Box::new(start),
                                    delete_count: Box::new(delete_count),
                                    items,
                                }));
                            }
                            "with" => {
                                // Issue #515: only fold `arr.with(idx, val)` to
                                // `Expr::ArrayWith` when the receiver is statically
                                // typed as an array or typed-array. `with` is
                                // heavily overloaded by user-defined builder
                                // methods (`class Builder { with(a, b): this {
                                // … } }`, `const obj = { with(a, b) { … } }`);
                                // folding optimistically on unknown-type receivers
                                // (the default for unannotated locals) silently
                                // rewrote the call to a typed-array index-replace
                                // and broke user code. Untyped-but-actually-array
                                // callers fall through to the codegen
                                // `lower_array_method` `with` arm (when
                                // `is_array_expr` recognizes the receiver) or to
                                // the runtime `js_native_call_method` arm.
                                let recv_ty = ctx.lookup_local_type(&arr_name);
                                let is_typed_array = recv_ty
                                    .as_ref()
                                    .map(|ty| {
                                        matches!(ty, Type::Named(n) if matches!(
                                            n.as_str(),
                                            "Int8Array" | "Int16Array" | "Int32Array"
                                            | "Uint8Array" | "Uint8ClampedArray"
                                            | "Uint16Array" | "Uint32Array"
                                            | "Float16Array" | "Float32Array" | "Float64Array"
                                            | "BigInt64Array" | "BigUint64Array"
                                        ))
                                    })
                                    .unwrap_or(false);
                                let is_known_array = is_typed_array
                                    || recv_ty
                                        .map(|ty| matches!(ty, Type::Array(_) | Type::Tuple(_)))
                                        .unwrap_or(false);
                                if is_known_array && args.len() >= 2 {
                                    let mut args_iter = args.into_iter();
                                    let index = args_iter.next().unwrap();
                                    let value = args_iter.next().unwrap();
                                    return Ok(Ok(Expr::ArrayWith {
                                        array: Box::new(Expr::LocalGet(array_id)),
                                        index: Box::new(index),
                                        value: Box::new(value),
                                    }));
                                }
                                // Fall through to general method dispatch
                            }
                            "copyWithin" => {
                                // #2879: typed-array receivers must NOT fold to
                                // `Expr::ArrayCopyWithin` — that path treats the
                                // receiver as an `ArrayHeader` with boxed f64 slots,
                                // which is invalid for `TypedArrayHeader` raw
                                // storage. Fall through so they reach the runtime
                                // `js_typed_array_copy_within` arm in
                                // `js_native_call_method`.
                                let is_typed_array = ctx
                                    .lookup_local_type(&arr_name)
                                    .map(|ty| {
                                        matches!(ty, Type::Named(n) if matches!(
                                            n.as_str(),
                                            "Int8Array" | "Int16Array" | "Int32Array"
                                            | "Uint8Array" | "Uint8ClampedArray"
                                            | "Uint16Array" | "Uint32Array"
                                            | "Float16Array" | "Float32Array" | "Float64Array"
                                            | "BigInt64Array" | "BigUint64Array"
                                        ))
                                    })
                                    .unwrap_or(false);
                                if !is_typed_array && args.len() >= 2 {
                                    let mut args_iter = args.into_iter();
                                    let target = args_iter.next().unwrap();
                                    let start = args_iter.next().unwrap();
                                    let end = args_iter.next().map(Box::new);
                                    return Ok(Ok(Expr::ArrayCopyWithin {
                                        array_id,
                                        target: Box::new(target),
                                        start: Box::new(start),
                                        end,
                                    }));
                                }
                            }
                            "entries" | "keys" | "values" => {
                                // Issue #542/#543: `keys()`/`values()`/`entries()` are
                                // shared between Array, Map, and Set. When the receiver's
                                // static type is Any (e.g. an interface method return
                                // whose signature wasn't tracked, or a `JSON.parse`
                                // result), the optimistic fall-through to `ArrayKeys`/
                                // `ArrayValues`/`ArrayEntries` runs `js_array_*` against
                                // a real `MapHeader`. The map's `size` field aliases
                                // `ArrayHeader.length`, so an N-entry Map produces
                                // `[0..N-1]` for `keys()` and reads garbage from the
                                // entries pointer for `values()`/`entries()`. Only fold
                                // to the Array variant when the receiver is statically
                                // known to be Array/Tuple; otherwise leave as a generic
                                // method call so codegen routes through
                                // `js_native_call_method`, which does the runtime
                                // is_registered_map / is_registered_set check.
                                let recv_ty = ctx.lookup_local_type(&arr_name);
                                // Issue #542/#543 follow-up: accept `Type::Union` variants
                                // containing the target (e.g. `Map<K,V> | undefined` after
                                // an `if (!m) return;` narrow). The for-of path in lower.rs
                                // already handles Union; the method-call path here did not,
                                // so `m.keys()` on an optional-Map fell through to the
                                // ArrayKeys fold. Mirrors lower.rs:6196.
                                let ty_is_map = |t: &Type| matches!(t, Type::Generic { base, .. } if base == "Map" || base == "WeakMap");
                                let ty_is_set = |t: &Type| matches!(t, Type::Generic { base, .. } if base == "Set" || base == "WeakSet");
                                let ty_is_array =
                                    |t: &Type| matches!(t, Type::Array(_) | Type::Tuple(_));
                                let is_map = match &recv_ty {
                                    Some(t) if ty_is_map(t) => true,
                                    Some(Type::Union(variants)) => variants.iter().any(ty_is_map),
                                    _ => false,
                                };
                                let is_set = match &recv_ty {
                                    Some(t) if ty_is_set(t) => true,
                                    Some(Type::Union(variants)) => variants.iter().any(ty_is_set),
                                    _ => false,
                                };
                                let is_known_array = match &recv_ty {
                                    Some(t) if ty_is_array(t) => true,
                                    Some(Type::Union(variants)) => variants.iter().any(ty_is_array),
                                    _ => false,
                                };
                                // #2856: Map/Set `entries`/`keys`/`values`
                                // are NOT folded to the Array-materializing
                                // `Map*`/`SetValues` HIR variants here. Those
                                // variants are reserved for the for-of /
                                // spread fast paths (which iterate the
                                // collection directly); a *value-level* call
                                // must return a real iterator OBJECT. Letting
                                // these fall through to general dispatch routes
                                // them to codegen's `is_map_expr`/`is_set_expr`
                                // branch → `js_*_iter_obj`. The `is_map`/
                                // `is_set` flags are still computed above so
                                // the array fold below doesn't claim them.
                                let _ = (is_map, is_set);
                                match method_name {
                                    "entries" => {
                                        if !is_map && !is_set && is_known_array {
                                            return Ok(Ok(Expr::ArrayEntries(Box::new(
                                                Expr::LocalGet(array_id),
                                            ))));
                                        }
                                    }
                                    "keys" => {
                                        if !is_map && !is_set && is_known_array {
                                            return Ok(Ok(Expr::ArrayKeys(Box::new(
                                                Expr::LocalGet(array_id),
                                            ))));
                                        }
                                    }
                                    "values" => {
                                        if !is_map && !is_set && is_known_array {
                                            return Ok(Ok(Expr::ArrayValues(Box::new(
                                                Expr::LocalGet(array_id),
                                            ))));
                                        }
                                    }
                                    _ => unreachable!(),
                                }
                                // Fall through: Map/Set or unknown receiver —
                                // general dispatch (codegen Map/Set branch or
                                // runtime `js_native_call_method`) handles it.
                            }
                            // Map methods (only apply to actual Map/Set types)
                            "set" => {
                                // Check if this is a Map or Set type before treating as Map.set()
                                let is_map_or_set = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                        .unwrap_or(false);
                                if is_map_or_set && args.len() >= 2 {
                                    // map.set(key, value) - returns the map for chaining
                                    let mut args_iter = args.into_iter();
                                    let key = args_iter.next().unwrap();
                                    let value = args_iter.next().unwrap();
                                    return Ok(Ok(Expr::MapSet {
                                        map: Box::new(Expr::LocalGet(array_id)),
                                        key: Box::new(key),
                                        value: Box::new(value),
                                    }));
                                }
                            }
                            "get" => {
                                // Check if this is a Map type before treating as Map.get()
                                let is_map = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                        .unwrap_or(false);
                                if is_map && !args.is_empty() {
                                    // map.get(key) - returns value or undefined
                                    return Ok(Ok(Expr::MapGet {
                                        map: Box::new(Expr::LocalGet(array_id)),
                                        key: Box::new(args.into_iter().next().unwrap()),
                                    }));
                                }
                            }
                            "has" => {
                                // Check if this is a Set or Map - only apply to actual Set/Map types
                                let is_set = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                        .unwrap_or(false);
                                let is_map = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                        .unwrap_or(false);
                                if (is_set || is_map) && !args.is_empty() {
                                    let value = args.into_iter().next().unwrap();
                                    if is_set {
                                        return Ok(Ok(Expr::SetHas {
                                            set: Box::new(Expr::LocalGet(array_id)),
                                            value: Box::new(value),
                                        }));
                                    } else {
                                        return Ok(Ok(Expr::MapHas {
                                            map: Box::new(Expr::LocalGet(array_id)),
                                            key: Box::new(value),
                                        }));
                                    }
                                }
                            }
                            "delete" => {
                                // Check if this is a Set or Map - only apply to actual Set/Map types
                                let is_set = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                        .unwrap_or(false);
                                let is_map = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                        .unwrap_or(false);
                                if (is_set || is_map) && !args.is_empty() {
                                    let value = args.into_iter().next().unwrap();
                                    if is_set {
                                        return Ok(Ok(Expr::SetDelete {
                                            set: Box::new(Expr::LocalGet(array_id)),
                                            value: Box::new(value),
                                        }));
                                    } else {
                                        return Ok(Ok(Expr::MapDelete {
                                            map: Box::new(Expr::LocalGet(array_id)),
                                            key: Box::new(value),
                                        }));
                                    }
                                }
                            }
                            "clear" => {
                                // Check if this is a Set or Map - only apply to actual Set/Map types
                                let is_set = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                        .unwrap_or(false);
                                let is_map = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map"))
                                        .unwrap_or(false);
                                if is_set {
                                    return Ok(Ok(Expr::SetClear(Box::new(Expr::LocalGet(
                                        array_id,
                                    )))));
                                } else if is_map {
                                    return Ok(Ok(Expr::MapClear(Box::new(Expr::LocalGet(
                                        array_id,
                                    )))));
                                }
                                // Fall through if neither Set nor Map
                            }
                            // #853: the `"entries" | "keys" | "values"` arm earlier
                            // in this match (around line 4323) already dispatches
                            // Map/Set/Array iterator methods for every receiver type.
                            // The Map-only duplicate arms that used to live here were
                            // dead under that arm's coverage — removed.
                            // Set methods
                            "add" => {
                                // Check if this is a Set type before treating as Set.add()
                                let is_set = ctx.lookup_local_type(&arr_name)
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Set"))
                                        .unwrap_or(false);
                                if is_set && !args.is_empty() {
                                    // set.add(value) - returns the set for chaining
                                    let value = args.into_iter().next().unwrap();
                                    return Ok(Ok(Expr::SetAdd {
                                        set_id: array_id,
                                        value: Box::new(value),
                                    }));
                                }
                            }
                            _ => {} // Fall through to generic handling
                        }

                        // URLSearchParams methods
                        let is_url_search_params = ctx
                            .lookup_local_type(&arr_name)
                            .map(|ty| matches!(ty, Type::Named(name) if name == "URLSearchParams"))
                            .unwrap_or(false);
                        if is_url_search_params {
                            match build_url_search_params_method_call(
                                Expr::LocalGet(array_id),
                                method_name,
                                args,
                            ) {
                                Ok(expr) => return Ok(Ok(expr)),
                                Err(returned_args) => args = returned_args,
                            }
                        }

                        // TextEncoder methods
                        let is_text_encoder = ctx
                            .lookup_local_type(&arr_name)
                            .map(|ty| matches!(ty, Type::Named(name) if name == "TextEncoder"))
                            .unwrap_or(false);
                        if is_text_encoder {
                            if method_name == "encode" {
                                if !args.is_empty() {
                                    return Ok(Ok(Expr::TextEncoderEncode(Box::new(
                                        args.into_iter().next().unwrap(),
                                    ))));
                                } else {
                                    // encode() with no args encodes empty string
                                    return Ok(Ok(Expr::TextEncoderEncode(Box::new(
                                        Expr::String(String::new()),
                                    ))));
                                }
                            }
                            if method_name == "encodeInto" {
                                let mut args = args.into_iter();
                                let source =
                                    args.next().unwrap_or_else(|| Expr::String(String::new()));
                                let dest = args.next().unwrap_or(Expr::Undefined);
                                return Ok(Ok(Expr::TextEncoderEncodeInto {
                                    source: Box::new(source),
                                    dest: Box::new(dest),
                                }));
                            }
                        }

                        // TextDecoder methods
                        let is_text_decoder = ctx
                            .lookup_local_type(&arr_name)
                            .map(|ty| matches!(ty, Type::Named(name) if name == "TextDecoder"))
                            .unwrap_or(false);
                        if is_text_decoder && method_name == "decode" {
                            let decoder = lower_expr(ctx, &member.obj)?;
                            let input = if !args.is_empty() {
                                args.into_iter().next().unwrap()
                            } else {
                                Expr::Undefined
                            };
                            return Ok(Ok(Expr::TextDecoderDecode {
                                decoder: Box::new(decoder),
                                input: Box::new(input),
                            }));
                        }
                    }
                } // close is_array_type check
            }

            // Check for array methods on property access (e.g., this.items.push(value))
            // This handles cases where the array is a property of an object, not a local variable
            if let ast::Expr::Member(obj_member) = member.obj.as_ref() {
                if let ast::MemberProp::Ident(obj_prop_ident) = &obj_member.prop {
                    let _property_name = obj_prop_ident.sym.to_string();
                    // Lower the object expression (e.g., 'this' or a local variable)
                    let _object_expr = lower_expr(ctx, &obj_member.obj)?;

                    if method_name == "push" && !args.is_empty() {
                        // For now, fall through to generic Call handling
                        // We'll compile this in codegen using inline property access
                        // property-based push: object.{property}.push()
                    }
                }
            }
        }
    }
    Ok(Err(args))
}
