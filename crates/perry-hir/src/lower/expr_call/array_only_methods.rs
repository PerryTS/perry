//! Array-only method dispatch on arbitrary expressions (Object.entries(x).reduce(...) etc.).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

use crate::ir::*;

/// Is `expr` a reference to a node:stream class constructor — bare
/// (`Readable`) or namespaced (`stream.Readable`)? Used by
/// [`chain_roots_at_stream`].
fn unwrap_transparent_expr(expr: &ast::Expr) -> &ast::Expr {
    let mut cur = expr;
    loop {
        match cur {
            ast::Expr::Paren(p) => cur = &p.expr,
            ast::Expr::TsAs(ts_as) => cur = &ts_as.expr,
            ast::Expr::TsNonNull(ts_non_null) => cur = &ts_non_null.expr,
            ast::Expr::TsSatisfies(ts_satisfies) => cur = &ts_satisfies.expr,
            ast::Expr::TsTypeAssertion(ts_type_assertion) => cur = &ts_type_assertion.expr,
            ast::Expr::TsConstAssertion(ts_const_assertion) => cur = &ts_const_assertion.expr,
            _ => return cur,
        }
    }
}

fn is_stream_class_ref(expr: &ast::Expr) -> bool {
    let expr = unwrap_transparent_expr(expr);
    let name = match expr {
        ast::Expr::Ident(i) => i.sym.as_ref(),
        ast::Expr::Member(m) => match &m.prop {
            ast::MemberProp::Ident(p) => p.sym.as_ref(),
            _ => return false,
        },
        _ => return false,
    };
    matches!(name, "Readable" | "Duplex" | "Transform" | "PassThrough")
}

fn is_node_readable_receiver(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Ident(ident) = unwrap_transparent_expr(expr) else {
        return false;
    };
    let name = ident.sym.as_ref();
    ctx.lookup_native_instance(name)
        .is_some_and(|(module, class_name)| {
            matches!(module, "stream" | "node:stream")
                && matches!(
                    class_name,
                    "Readable" | "Duplex" | "Transform" | "PassThrough"
                )
        })
        || ctx.lookup_local_type(name).is_some_and(|ty| {
            matches!(
                ty,
                Type::Named(class_name)
                    if matches!(
                        class_name.as_str(),
                        "Readable" | "Duplex" | "Transform" | "PassThrough"
                    )
            )
        })
}

fn is_module_builtin_modules_expr(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Member(member) = unwrap_transparent_expr(expr) else {
        return false;
    };
    let ast::MemberProp::Ident(prop_ident) = &member.prop else {
        return false;
    };
    if prop_ident.sym.as_ref() != "builtinModules" {
        return false;
    }
    let ast::Expr::Ident(obj_ident) = unwrap_transparent_expr(member.obj.as_ref()) else {
        return false;
    };
    let obj_name = obj_ident.sym.as_ref();
    obj_name == "module"
        || ctx.lookup_builtin_module_alias(obj_name) == Some("module")
        || ctx
            .lookup_native_module(obj_name)
            .map(|(module_name, _)| module_name == "module")
            .unwrap_or(false)
}

fn is_util_or_sys(module_name: &str) -> bool {
    matches!(module_name, "util" | "sys")
}

fn is_util_mime_constructor(ctx: &LoweringContext, expr: &ast::Expr, class_name: &str) -> bool {
    match unwrap_transparent_expr(expr) {
        ast::Expr::Ident(ident) => ctx
            .lookup_native_module(ident.sym.as_ref())
            .map(|(module_name, method_name)| {
                is_util_or_sys(module_name) && method_name == Some(class_name)
            })
            .unwrap_or(false),
        ast::Expr::Member(member) => {
            let ast::MemberProp::Ident(prop_ident) = &member.prop else {
                return false;
            };
            if prop_ident.sym.as_ref() != class_name {
                return false;
            }
            let ast::Expr::Ident(obj_ident) = unwrap_transparent_expr(member.obj.as_ref()) else {
                return false;
            };
            ctx.lookup_native_module(obj_ident.sym.as_ref())
                .map(|(module_name, method_name)| {
                    is_util_or_sys(module_name) && method_name.is_none()
                })
                .unwrap_or(false)
                || ctx
                    .lookup_builtin_module_alias(obj_ident.sym.as_ref())
                    .map(is_util_or_sys)
                    .unwrap_or(false)
        }
        _ => false,
    }
}

fn is_util_mime_instance(ctx: &LoweringContext, expr: &ast::Expr, class_name: &str) -> bool {
    match unwrap_transparent_expr(expr) {
        ast::Expr::Ident(ident) => ctx
            .lookup_native_instance(ident.sym.as_ref())
            .map(|(module_name, instance_class)| {
                is_util_or_sys(module_name) && instance_class == class_name
            })
            .unwrap_or(false),
        ast::Expr::New(new_expr) => is_util_mime_constructor(ctx, &new_expr.callee, class_name),
        _ => false,
    }
}

fn is_util_mime_params_receiver(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match unwrap_transparent_expr(expr) {
        ast::Expr::Ident(_) | ast::Expr::New(_) => is_util_mime_instance(ctx, expr, "MIMEParams"),
        ast::Expr::Member(member) => {
            let ast::MemberProp::Ident(prop_ident) = &member.prop else {
                return false;
            };
            prop_ident.sym.as_ref() == "params"
                && is_util_mime_instance(ctx, &member.obj, "MIMEType")
        }
        _ => false,
    }
}

fn is_fs_dir_receiver(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let ast::Expr::Ident(ident) = unwrap_transparent_expr(expr) else {
        return false;
    };
    ctx.lookup_local_type(ident.sym.as_ref())
        .is_some_and(|ty| matches!(ty, Type::Named(name) if name == "Dir" || name == "fs.Dir"))
        || ctx
            .lookup_native_instance(ident.sym.as_ref())
            .is_some_and(|(module, class_name)| {
                module.strip_prefix("node:").unwrap_or(module) == "fs" && class_name == "Dir"
            })
}

/// Does this expression's method chain originate from a node:stream
/// source — `Readable.from(...)` / `Readable.of(...)`, a local already tagged
/// as a readable stream, `new Transform()`, or a chain of lazy iterator helpers
/// (`map`/`filter`/`flatMap`/`take`/`drop`) on top of one? (#1558)
///
/// The lazy stream helpers return another Readable, not an array, so a
/// chain like `Readable.from(x).map(f).filter(g)` must NOT be folded
/// into `Expr::Array<Method>` ops — `js_array_map` would read garbage
/// out of the stream object's header. Detecting the stream root here
/// keeps such chains on dynamic dispatch so the runtime's stream
/// iterator-helper stubs run.
fn chain_roots_at_stream(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    let expr = unwrap_transparent_expr(expr);
    if is_node_readable_receiver(ctx, expr) {
        return true;
    }
    match expr {
        ast::Expr::Await(a) => chain_roots_at_stream(ctx, &a.arg),
        ast::Expr::New(new) => is_stream_class_ref(&new.callee),
        ast::Expr::Call(call) => {
            let ast::Callee::Expr(callee) = &call.callee else {
                return false;
            };
            let ast::Expr::Member(m) = callee.as_ref() else {
                return false;
            };
            let ast::MemberProp::Ident(prop) = &m.prop else {
                return false;
            };
            match prop.sym.as_ref() {
                // Static factories that produce a Readable.
                "from" | "of" => is_stream_class_ref(&m.obj),
                // Lazy helpers preserve the stream — recurse into the receiver.
                "map" | "filter" | "flatMap" | "take" | "drop" => {
                    chain_roots_at_stream(ctx, &m.obj)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// #2874: does this expression's method chain originate from
/// `Iterator.from(...)` (optionally through lazy helpers `map`/`filter`/
/// `take`/`drop`/`flatMap`)? Such a chain yields a lazy iterator-helper
/// OBJECT, not an array, so it must NOT be folded into `Expr::Array<Method>`
/// ops — `js_array_map` would read garbage out of the helper's header. Keep it
/// on dynamic dispatch so `dispatch_iterator_helper_method` runs. Mirrors
/// `chain_roots_at_stream`; AST-only (no type info), so it catches the common
/// inline-chain form.
fn chain_roots_at_iterator_from(expr: &ast::Expr) -> bool {
    let expr = unwrap_transparent_expr(expr);
    let ast::Expr::Call(call) = expr else {
        return false;
    };
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(m) = callee.as_ref() else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &m.prop else {
        return false;
    };
    match prop.sym.as_ref() {
        // The static factory `Iterator.from(x)` is the root.
        "from" => matches!(
            unwrap_transparent_expr(m.obj.as_ref()),
            ast::Expr::Ident(i) if i.sym.as_ref() == "Iterator"
        ),
        // Lazy helpers preserve the helper — recurse into the receiver.
        "map" | "filter" | "take" | "drop" | "flatMap" => chain_roots_at_iterator_from(&m.obj),
        _ => false,
    }
}

use super::super::{lower_expr, LoweringContext};

/// Wall 17 (nestjs / iterare): is `expr` statically KNOWN to be an Array?
///
/// Used by the chained-call array-method fold to decide whether a
/// `recv.<m>(...)` whose `recv` is itself a call (`inner.<im>(...)`) is a
/// genuine array chain (`[1,2,3].map(f).map(g)`) or a USER iterator chain
/// whose `.map`/`.filter` are user methods returning a class instance
/// (iterare's `IteratorWithOperators`, RxJS-likes). The AST-only inner-method-
/// name check (`map`/`filter` "produce arrays") was OPTIMISTIC and mis-folded
/// `iterate(x).map(f).map(g)` into `Expr::ArrayMap`, so the outer `new
/// IteratorWithOperators(...)` was constructed by `js_array_map` (returning an
/// empty/garbage array, GC-typed ARRAY) → "neither Iterator nor Iterable".
///
/// We only return `true` when the chain provably ROOTS at an array: an array
/// literal, an array-typed local, `Array.from/of(...)`, `Object.entries/keys/
/// values(...)`, or recursively another array-rooted producer call. An unknown
/// / `Any` / user-class root yields `false`, routing the outer method to the
/// runtime's GC-type-checked dynamic dispatch — which correctly runs
/// `js_array_map` for real arrays and the user method for class instances.
fn chain_roots_at_array(ctx: &LoweringContext, expr: &ast::Expr) -> bool {
    match unwrap_transparent_expr(expr) {
        // `[...]` literal.
        ast::Expr::Array(_) => true,
        // A local declared/inferred as `T[]` / tuple.
        ast::Expr::Ident(ident) => matches!(
            ctx.lookup_local_type(ident.sym.as_ref()),
            Some(Type::Array(_)) | Some(Type::Tuple(_))
        ),
        // A producer call.
        ast::Expr::Call(call) => call_roots_at_array(ctx, call),
        _ => false,
    }
}

/// `CallExpr` form of [`chain_roots_at_array`] (the inner-call arm hands us a
/// `&CallExpr`, not an `&Expr`).
fn call_roots_at_array(ctx: &LoweringContext, call: &ast::CallExpr) -> bool {
    let ast::Callee::Expr(callee) = &call.callee else {
        return false;
    };
    let ast::Expr::Member(m) = unwrap_transparent_expr(callee.as_ref()) else {
        return false;
    };
    let ast::MemberProp::Ident(prop) = &m.prop else {
        return false;
    };
    let method = prop.sym.as_ref();
    let recv = unwrap_transparent_expr(m.obj.as_ref());
    // `Array.from(x)` / `Array.of(x)` always yield a fresh array.
    if matches!(method, "from" | "of")
        && matches!(recv, ast::Expr::Ident(i) if i.sym.as_ref() == "Array")
    {
        return true;
    }
    // `Object.entries/keys/values(x)` always yield a fresh array.
    if matches!(method, "entries" | "keys" | "values")
        && matches!(recv, ast::Expr::Ident(i) if i.sym.as_ref() == "Object")
    {
        return true;
    }
    // An array-producing prototype method roots at an array only if ITS OWN
    // receiver provably roots at an array (recurse). `map` / `filter` /
    // `flatMap` are intentionally included here ONLY when so rooted — that is
    // what distinguishes `[1].map(f).map(g)` (root is a literal) from
    // `iterate(x).map(f).map(g)` (root is a user call).
    let array_producing = matches!(
        method,
        "map"
            | "filter"
            | "slice"
            | "concat"
            | "flat"
            | "flatMap"
            | "splice"
            | "sort"
            | "reverse"
            | "fill"
            | "copyWithin"
            | "toReversed"
            | "toSorted"
            | "toSpliced"
            | "with"
    );
    array_producing && chain_roots_at_array(ctx, m.obj.as_ref())
}

pub(super) fn try_array_only_methods(
    ctx: &mut LoweringContext,
    call: &ast::CallExpr,
    args: Vec<Expr>,
) -> Result<Result<Expr, Vec<Expr>>> {
    // Check for array-only methods on any expression (e.g., Object.entries(x).reduce(...))
    // ONLY match methods that are unique to arrays (not shared with strings)
    // "includes", "indexOf", "slice", "join" also exist on strings, so skip those
    if let ast::Callee::Expr(expr) = &call.callee {
        if let ast::Expr::Member(member) = expr.as_ref() {
            if let ast::MemberProp::Ident(method_ident) = &member.prop {
                let method_name = method_ident.sym.as_ref();
                // #6718: the `args` vector holds the EVALUATED spread expression
                // with the spread token dropped, so any arm reading a positional
                // slot (callback, comparator, index, search value, …) would bind
                // the spread SOURCE array itself — `Object.keys(x).map(...[fn])` →
                // "object is not a function", `expr.slice(...[1,3])` → the whole
                // array. Decline on any spread so the generic tail builds an
                // `Expr::CallSpread`, whose member-callee arm flattens the spread
                // into one argument array and dispatches the array method by name
                // with `this` bound to the receiver (`js_native_call_method_apply_by_id`).
                // `push` is excluded: it has a dedicated spread-aware arm below
                // (`push_spread`) that materializes the spread correctly.
                if call.args.iter().any(|a| a.spread.is_some()) && method_name != "push" {
                    return Ok(Err(args));
                }
                // Helper: skip array-method dispatch when the receiver is a
                // known class instance (e.g. mongo `Collection.find`,
                // `Stack<T>.map`). Without this guard the lowering blindly
                // emits `Expr::Array<Method>` and the compiled binary calls
                // `js_array_<method>` on a class handle.
                let member_obj = unwrap_transparent_expr(member.obj.as_ref());
                let recv_is_known_string_prop = matches!(
                    member_obj,
                    ast::Expr::Member(inner)
                        if matches!(
                            &inner.prop,
                            ast::MemberProp::Ident(prop)
                                if matches!(
                                    prop.sym.as_ref(),
                                    "stack" | "message" | "name" | "sourceSQL" | "expandedSQL"
                                )
                        )
                );
                let recv_is_class = match member_obj {
                    ast::Expr::Ident(ident) => {
                        let n = ident.sym.to_string();
                        // #39 (#321): a module namespace import
                        // (`import * as NS from "..."`) is NOT an array. Its
                        // exports are member functions, so `NS.sort(cmp)` /
                        // `NS.reverse()` / `NS.entries()` etc. must dispatch the
                        // namespace export, never `js_array_<method>` on the
                        // namespace object. effect's `Array.sort = dual(2, ...)`
                        // is data-last: `Arr.sort(numberOrder)` should return the
                        // curried `(self) => ...` closure. The non-overlapping
                        // mutators (`sort`/`reverse`/`entries`/`keys`/`values`/
                        // `splice`/`fill`/…) bypass the `is_overlapping` gate
                        // below and the `"sort" if !args.is_empty()` arm folds
                        // `NS.sort(cmp)` to `Expr::ArraySort { array: NS, ... }`,
                        // making the compiled binary call `js_array_sort(NS, cmp)`
                        // — which returned the namespace object itself (typeof
                        // "object"). The downstream `pipe(bounds, sortFn, ...)`
                        // then threw "value is not a function" inside effect's
                        // metric histogram path at fiber exit. `try_imported_
                        // array_methods` already guards namespaces, but it runs
                        // *before* this pass and returns `Err(args)`, so the call
                        // re-enters here; guard at the top of the Ident arm and
                        // bail (treat as not-an-array) for ALL method names when
                        // the receiver is a namespace import.
                        if ctx.namespace_import_locals.contains(&n) {
                            return Ok(Err(args));
                        }
                        // A default/CJS-module import binding used as a method
                        // receiver (`import semver from "semver"; semver.sort(x)`)
                        // is a module-exports object, never a local array. It
                        // lowers to an `ExternFuncRef` receiver, so the array-
                        // method fold would mis-route `semver.sort(list)` into
                        // `Expr::ArraySort { array: semver, comparator: list }`
                        // (the single `list` arg landing in the comparator slot →
                        // "comparison function must be either a function or
                        // undefined"). Treat imported bindings like namespaces and
                        // bail to the namespace/object method dispatch.
                        if ctx.lookup_local(&n).is_none() && ctx.lookup_imported_func(&n).is_some()
                        {
                            return Ok(Err(args));
                        }
                        let ty = ctx.lookup_local_type(&n);
                        let class_typed = ty
                            .as_ref()
                            .map(|t| {
                                matches!(t, Type::Named(_) | Type::Generic { .. })
                                    && !matches!(t, Type::Array(_))
                            })
                            .unwrap_or(false);
                        let unknown_recv =
                            matches!(ty, None | Some(Type::Any) | Some(Type::Unknown));
                        let is_overlapping = matches!(
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
                                // wall 49: mutating array methods are also common
                                // user-class methods (Stack.push, Queue.shift,
                                // Next.js DefaultRouteMatcherManager.push). On an
                                // unknown (`Any`) receiver, folding to the array
                                // op corrupts a class instance (its ObjectHeader is
                                // read as an ArrayHeader). Bail to dynamic dispatch;
                                // real arrays are `Type::Array`, handled by the
                                // class_typed=false + typed fast paths elsewhere.
                                | "push"
                                | "pop"
                                | "shift"
                                | "unshift"
                        );
                        class_typed || (unknown_recv && is_overlapping)
                    }
                    ast::Expr::New(_) => true,
                    // `this.<method>(...)` inside a class method.
                    // The receiver is the class instance; treat it
                    // like a class-typed local. Without this, calls
                    // with names that overlap an Array method
                    // (e.g. a user-defined `this.forEach(...)` in
                    // an ECS Archetype, `this.map(...)` on a Stack)
                    // got folded into Expr::ArrayForEach /
                    // Expr::ArrayMap and the compiled binary
                    // dispatched `js_array_*` on a class handle.
                    // For non-overlapping names there's nothing to
                    // hijack, so we don't need to check the
                    // current_class field list — the array-method
                    // arms simply don't fire.
                    ast::Expr::This(_) => true,
                    // `this.<field>.<method>(...)` — when the field
                    // is statically typed as a non-Array container
                    // (Map / Set / a user class), the array-method
                    // fold would silently rewrite the call to
                    // `Expr::Array<Method>` and codegen would
                    // dispatch `js_array_*` on the wrong header.
                    // Concretely: an ECS Archetype's
                    // `private componentData: Map<EntityId, any[]>`
                    // ends up with `this.componentData.forEach(cb)`
                    // hitting `js_array_forEach`, which reads
                    // "length" out of a Map header and returns the
                    // map's `size` field as a fake array length,
                    // then iterates with bogus index-based reads
                    // (which is why `forEach` reported keys
                    // `[0,1,2]` while the real Map keys were
                    // 1024-range EntityIds).
                    //
                    // Resolve the field's type via the class field
                    // registry (populated by `lower_class_decl`'s
                    // first pass) and treat anything that isn't
                    // declared `Array<T>` / `T[]` as a class
                    // receiver so the fold bails. Unknown / Any
                    // stays in the prior `false` arm — no info to
                    // act on, fall through to the existing fast
                    // path.
                    ast::Expr::Member(inner) => {
                        if let (ast::Expr::This(_), ast::MemberProp::Ident(p)) =
                            (inner.obj.as_ref(), &inner.prop)
                        {
                            if let Some(cls) = ctx.current_class.clone() {
                                match ctx.lookup_class_field_type(&cls, p.sym.as_ref()) {
                                    Some(Type::Array(_)) | Some(Type::Tuple(_)) => false,
                                    Some(_) => true,
                                    None => false,
                                }
                            } else {
                                false
                            }
                        } else if let (
                            ast::Expr::Ident(obj_ident),
                            ast::MemberProp::Ident(prop_ident),
                        ) = (inner.obj.as_ref(), &inner.prop)
                        {
                            // Closes #589 (runtime path): `r.headers.forEach(cb)`
                            // where `r` is a Response/Request (or returned Headers
                            // already). Without this guard, the array-method fold
                            // catch-all rewrites the call to `Expr::ArrayForEach`
                            // and codegen dispatches `js_array_forEach` against a
                            // Headers handle — Headers iteration silently no-ops.
                            // Bail to the chained-Web-Fetch dispatch in
                            // `crates/perry-codegen/src/lower_call.rs:1313`, which
                            // routes `.forEach` / `.get` / `.has` / `.keys` /
                            // `.values` / `.entries` through the Headers FFI.
                            // #5432: `res.headers.<m>()` where `res` came from a
                            // member-call `app.fetch(req)` (Hono / WinterCG). It
                            // is not a `register_native_instance` binding (that
                            // would hijack every method on `res`), so consult the
                            // narrow `fetch_call_response_locals` set too.
                            let is_fetch_headers = prop_ident.sym.as_ref() == "headers"
                                && (matches!(
                                    ctx.lookup_native_instance(obj_ident.sym.as_ref()),
                                    Some(("fetch", _))
                                        | Some(("Request", _))
                                        | Some(("Headers", _))
                                ) || ctx
                                    .fetch_call_response_locals
                                    .contains(obj_ident.sym.as_ref()));
                            is_fetch_headers
                        } else {
                            false
                        }
                    }
                    // Issue #528: chained `inner_call(...).<overlapping>(...)`.
                    // The previous catch-all `_ => false` let the array-method
                    // fold fire on ANY chained call, including ones whose inner
                    // call returns a user object — `this.col().find({})` got
                    // rewritten as `Expr::ArrayFind(this.col(), {})` and at
                    // runtime `js_array_find` read garbage out of the object's
                    // header (no fallback path: only `map`/`filter`/`forEach`/
                    // `slice`/`with` have arms in `js_native_call_method`'s
                    // array dispatch tower; `find`/`findIndex`/`reduce` do not).
                    //
                    // For overlapping methods (the same set the Ident arm
                    // gates on), bail unless the inner call's method name is
                    // one of the known array-producing builtins. The AST-level
                    // check is conservative: it doesn't catch every chained-
                    // array shape (e.g. `Array.from(x).find(p)` — `Array` is
                    // an Ident, not a method on a Member), but it DOES catch
                    // the common `arr.filter(p).find(q)` chain whose inner
                    // call IS a Member-on-something. Non-overlapping methods
                    // (slice/indexOf/includes/etc.) keep their existing
                    // positive inner-HIR-shape pattern at lines ~3950+.
                    ast::Expr::Call(inner_call) => {
                        let is_overlapping = matches!(
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
                        );
                        if !is_overlapping {
                            false
                        } else {
                            // Wall 17 (nestjs / iterare): the previous heuristic
                            // looked ONLY at the inner call's METHOD NAME — if it
                            // was `map`/`filter`/`slice`/… it optimistically
                            // assumed an array chain and folded the outer call to
                            // `Expr::ArrayMap`. But `map`/`filter`/`flatMap` are
                            // ALSO common USER iterator methods: iterare's
                            // `iterate(x).map(f).map(g)` returns an
                            // `IteratorWithOperators`, not an array, so the fold
                            // built the outer wrapper via `js_array_map` →
                            // empty/garbage ARRAY-typed object → downstream
                            // `Array.from(this)` saw "neither Iterator nor
                            // Iterable" and `getNonTransientInstances` died in
                            // `onModuleInit`.
                            //
                            // Fold to the array op ONLY when the inner chain
                            // PROVABLY roots at an array (literal / array-typed
                            // local / `Array.from`/`Object.entries` / a nested
                            // array-producer over such a root). Otherwise BAIL
                            // (recv_is_class = true) to the runtime's
                            // GC-type-checked dynamic dispatch, which runs
                            // `js_array_map` for real arrays and the user method
                            // for class instances — correct for BOTH.
                            !call_roots_at_array(ctx, inner_call)
                        }
                    }
                    _ => false,
                };
                // #1558: a node:stream chain (`Readable.from(x).map(f).filter(g)`)
                // must not be folded into Array<Method> ops — the lazy stream
                // transforms return a Readable, not an array, so `js_array_map`
                // would read garbage out of the stream object's header. Bail to
                // dynamic dispatch so the runtime's iterator-helper stubs run.
                let recv_is_class = recv_is_class
                    || chain_roots_at_stream(ctx, member_obj)
                    || chain_roots_at_iterator_from(member_obj)
                    || is_util_mime_params_receiver(ctx, member_obj);
                // thisArg routing: the dense `Expr::Array<Method>` fast paths
                // carry only the callback and silently drop a 2nd positional
                // `thisArg` argument, so `[x].every(cb, thisArg)` ran the
                // callback with `this === undefined` (test262 §15.4.4.* `-5-*`
                // / `-7-*`). The generic `Expr::ArrayLikeMethod` lowering binds
                // the callback `this` via the spec-complete `js_arraylike_*`
                // runtime entry points (ToObject + LengthOfArrayLike + a
                // ThisGuard around each invocation), so when an explicit thisArg
                // is supplied route the callback iterators through it. Gated on
                // `!recv_is_class` (which already excludes Map/Set/class
                // receivers — those keep their own forEach contract) and on the
                // 2nd argument being a plain positional (no spread).
                // `recv_is_class` does NOT cover Map/Set/URLSearchParams locals,
                // which keep their own `forEach` contract (callback signature +
                // thisArg binding via `js_{map,set}_foreach`). Folding a 2-arg
                // `set.forEach(cb, thisArg)` into the array-like path here ran the
                // callback against an array view → zero iterations (test262
                // Set/Map forEach this-arg-explicit). Exclude them explicitly.
                let recv_is_non_array_collection = {
                    let is_nac = |ty: &Type| {
                        matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set")
                            || matches!(ty, Type::Named(n) if n == "URLSearchParams")
                    };
                    match member.obj.as_ref() {
                        ast::Expr::Ident(ident) => {
                            match ctx.lookup_local_type(ident.sym.as_ref()) {
                                Some(ty) if is_nac(ty) => true,
                                Some(Type::Union(variants)) => variants.iter().any(is_nac),
                                _ => false,
                            }
                        }
                        _ => false,
                    }
                };
                if !recv_is_class
                    && !recv_is_non_array_collection
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
                    let receiver = Box::new(lower_expr(ctx, &member.obj)?);
                    return Ok(Ok(Expr::ArrayLikeMethod {
                        method: method_name.to_string(),
                        receiver,
                        args,
                    }));
                }
                // #5139: an Array-mutator name that a plain object can also own
                // as a closure-valued property (`{ push(c) {…} }` passed as
                // `any` — react-dom/server's SSR `destination`). When the bare-
                // ident receiver's static type is unknown we cannot tell a real
                // array from such an object, so committing to `Expr::ArraySort` /
                // the `array.push_single` native arm here reads the object's
                // header as an `ArrayHeader` and corrupts it. Defer to the
                // runtime `js_native_call_method` dispatch, which selects by the
                // receiver's runtime shape (real array → dense helper; plain
                // object with an own callable of this name → that method). The
                // GC_TYPE_ARRAY arms in `js_native_call_method` keep real arrays
                // correct, so any-typed arrays still work (growth resolves via
                // the #233 forwarding pointer). Mirrors the same deferral in
                // `local_array_methods.rs` and the method set in
                // `array::try_object_arraylike_mutator`.
                if let ast::Expr::Ident(ident) = unwrap_transparent_expr(member.obj.as_ref()) {
                    let unknown_recv = matches!(
                        ctx.lookup_local_type(ident.sym.as_ref()),
                        None | Some(Type::Any) | Some(Type::Unknown)
                    );
                    if unknown_recv
                        && matches!(
                            method_name,
                            "push"
                                | "pop"
                                | "shift"
                                | "unshift"
                                | "reverse"
                                | "splice"
                                | "sort"
                                | "concat"
                        )
                    {
                        return Ok(Err(args));
                    }
                }
                // The guard above only inspects a *bare identifier* receiver. A property
                // receiver (`this._commands.push(c)`, `h.q.push(c)`) had no gate at all
                // and committed to the eager `array.push_single` native arm, which threads
                // the receiver through `js_array_push_f64` — that helper writes
                // `(*arr).length` and the element slot with no shape check, so a plain
                // object receiver has its ObjectHeader overwritten as an ArrayHeader: the
                // user's `push` never runs, the call yields a bogus length, and later field
                // reads on the object come back `undefined`. denque (mysql2's command
                // queue) is exactly this shape — `Denque.prototype.push` reached via
                // `this._commands` — so every queued command was silently dropped.
                //
                // Fold only when the receiver is *provably* an array; otherwise defer to
                // the runtime `js_native_call_method` dispatch, which selects on the
                // receiver's real shape (real array → dense helper, growth via the #233
                // forwarding pointer; object → its own method). The rest of the mutator
                // family already reaches that dispatch for property receivers.
                if method_name == "push" {
                    let recv_ty = crate::lower_types::infer_type_from_expr(&member.obj, ctx);
                    let provably_array = matches!(recv_ty, Type::Array(_))
                        || matches!(&recv_ty, Type::Generic { base, .. } if base == "Array");
                    if !provably_array {
                        return Ok(Err(args));
                    }
                }
                match method_name {
                    "reduce" if !args.is_empty() && !recv_is_class => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        let mut args_iter = args.into_iter();
                        let callback = args_iter.next().unwrap();
                        let initial = args_iter.next().map(Box::new);
                        return Ok(Ok(Expr::ArrayReduce {
                            array: Box::new(array_expr),
                            callback: Box::new(callback),
                            initial,
                        }));
                    }
                    "map" if !args.is_empty() && !recv_is_class => {
                        let cb = args.into_iter().next().unwrap();
                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayMap {
                            array: Box::new(array_expr),
                            callback: Box::new(cb),
                        }));
                    }
                    "filter" if !args.is_empty() && !recv_is_class => {
                        let cb = args.into_iter().next().unwrap();
                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayFilter {
                            array: Box::new(array_expr),
                            callback: Box::new(cb),
                        }));
                    }
                    "forEach" if !args.is_empty() && !recv_is_class => {
                        // Check if the receiver is a Map or Set - if so, don't use ArrayForEach
                        let is_map_or_set = if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                            ctx.lookup_local_type(ident.sym.as_ref())
                                        .map(|ty| matches!(ty, Type::Generic { base, .. } if base == "Map" || base == "Set"))
                                        .unwrap_or(false)
                        } else {
                            false
                        };
                        if !is_map_or_set {
                            let cb = args.into_iter().next().unwrap();
                            let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            return Ok(Ok(Expr::ArrayForEach {
                                array: Box::new(array_expr),
                                callback: Box::new(cb),
                            }));
                        }
                    }
                    "find" if !args.is_empty() && !recv_is_class => {
                        let cb = args.into_iter().next().unwrap();
                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayFind {
                            array: Box::new(array_expr),
                            callback: Box::new(cb),
                        }));
                    }
                    "findIndex" if !args.is_empty() && !recv_is_class => {
                        let cb = args.into_iter().next().unwrap();
                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayFindIndex {
                            array: Box::new(array_expr),
                            callback: Box::new(cb),
                        }));
                    }
                    "some" if !args.is_empty() && !recv_is_class => {
                        let cb = args.into_iter().next().unwrap();
                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArraySome {
                            array: Box::new(array_expr),
                            callback: Box::new(cb),
                        }));
                    }
                    "every" if !args.is_empty() && !recv_is_class => {
                        let cb = args.into_iter().next().unwrap();
                        let cb = ctx.maybe_wrap_builtin_callback(cb, &call.args[0]);
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayEvery {
                            array: Box::new(array_expr),
                            callback: Box::new(cb),
                        }));
                    }
                    // #597: arr.entries() / .keys() / .values() on
                    // any-typed receivers (`function f(arr: any) { for (const [i,v] of arr.entries()) ... }`).
                    // Pre-fix this fell through to a generic
                    // `js_native_call_method` dispatch that returned
                    // an iterator-shaped object whose `.length` was
                    // 0 / undefined, so the index-based for-of loop
                    // (the index lowering at lower_decl.rs:4445)
                    // saw `__arr_N.length === 0` and ran 0 times.
                    // The static-Array path already folds at
                    // line 3966 above; this catch-all extends the
                    // same fold to dynamic-receiver shapes —
                    // `js_array_entries` / `_keys` / `_values`
                    // tolerates non-array receivers (returns empty)
                    // so the lowered loop's behavior on non-array
                    // values matches Node's empty-iterator semantics.
                    // recv_is_class gating preserves user classes
                    // that happen to expose an `entries` method.
                    // Drizzle's `dialect.buildInsertQuery` uses
                    // `for (const [valueIndex, value] of values.entries())`
                    // where `values` arrives via destructuring of an
                    // any-typed function param.
                    "entries"
                        if args.is_empty()
                            && !recv_is_class
                            && !is_fs_dir_receiver(ctx, &member.obj) =>
                    {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayEntries(Box::new(array_expr))));
                    }
                    "keys" if args.is_empty() && !recv_is_class => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayKeys(Box::new(array_expr))));
                    }
                    "values" if args.is_empty() && !recv_is_class => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayValues(Box::new(array_expr))));
                    }
                    // `!recv_is_class` gate: semver re-exports
                    // `sort = (list) => list.sort(cmp)` and the driver calls
                    // `semver.sort(list)`. Without the guard the namespace/class
                    // receiver was folded to `Expr::ArraySort`, mis-routing the
                    // single `list` argument into the comparator slot ("comparison
                    // function must be either a function or undefined"). Mirrors
                    // the sibling `map`/`filter`/`reduce` arms.
                    "sort" if !args.is_empty() && !recv_is_class => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArraySort {
                            array: Box::new(array_expr),
                            comparator: Box::new(args.into_iter().next().unwrap()),
                        }));
                    }
                    "slice"
                        if args.len() <= 2 && is_module_builtin_modules_expr(ctx, &member.obj) =>
                    {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        let mut args_iter = args.into_iter();
                        let start = args_iter.next().unwrap_or(Expr::Number(0.0));
                        let end = args_iter.next();
                        return Ok(Ok(Expr::ArraySlice {
                            array: Box::new(array_expr),
                            start: Box::new(start),
                            end: end.map(Box::new),
                        }));
                    }
                    // .slice() exists on both Array and String, so we can only safely
                    // lower to ArraySlice when the receiver is definitely an
                    // array-producing expression (matches the indexOf/includes pattern
                    // below). Without this, `arr.sort(cb).slice(0, 5)` falls through to
                    // generic dynamic dispatch which corrupts the result — the inner
                    // ArraySort returns a real array pointer but the outer .slice goes
                    // through `js_native_call_method` which can't unwrap it properly,
                    // producing an "object" with the right .length but Array.isArray
                    // returns false and JSON.stringify segfaults.
                    "slice" if !args.is_empty() => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        // #1177: when the receiver is statically Buffer-producing
                        // (`Buffer.concat(...).slice(0,8)`, `Buffer.from(...).slice(...)`,
                        // or a chained `.slice(...).slice(...)`), the receiver type isn't
                        // an `Ident` so the buffer-method block above can't see it. The
                        // generic Call fallthrough then routes `.slice` through
                        // `js_native_call_method` which picks String.slice semantics on
                        // the NaN-boxed Buffer pointer — producing a "string" of length
                        // 8 with all bytes as spaces/undefined. Fold to `Expr::BufferSlice`
                        // here so codegen calls `js_buffer_slice` directly. Must come
                        // BEFORE the Array shapes below so a `BufferConcat`/`BufferFrom`/
                        // `BufferSlice` receiver is never misrouted to `Expr::ArraySlice`.
                        if matches!(
                            &array_expr,
                            Expr::BufferConcat(_)
                                | Expr::BufferConcatWithLength { .. }
                                | Expr::BufferFrom { .. }
                                | Expr::BufferSlice { .. }
                        ) {
                            let mut args_iter = args.into_iter();
                            let start = args_iter.next().unwrap();
                            let end = args_iter.next();
                            return Ok(Ok(Expr::BufferSlice {
                                buffer: Box::new(array_expr),
                                start: Some(Box::new(start)),
                                end: end.map(Box::new),
                            }));
                        }
                        if matches!(
                            &array_expr,
                            Expr::ArrayMap { .. } | Expr::ArrayFilter { .. } | Expr::ArraySort { .. } |
                                    Expr::ArraySlice { .. } | Expr::Array(_) | Expr::ArraySpread(_) |
                                    Expr::ArrayFrom(_) | Expr::ArrayFromArrayLikeHoley(_) |
                                    Expr::ArrayFromMapped { .. } |
                                    Expr::ArrayFlat { .. } | Expr::StringSplit(_, _) |
                                    Expr::ArrayToReversed { .. } | Expr::ArrayToSorted { .. } |
                                    Expr::ArrayToSpliced { .. } | Expr::ArrayWith { .. } |
                                    Expr::ArrayEntries(_) | Expr::ArrayKeys(_) | Expr::ArrayValues(_) |
                                    Expr::ObjectKeys(_) | Expr::ObjectValues(_) | Expr::ObjectEntries(_) |
                                    // `process.argv` is a `string[]`. Without this arm the
                                    // fallthrough picked String.slice semantics — so
                                    // `process.argv.slice(2)` returned a "string" whose
                                    // length was the argv count and whose elements were
                                    // NaN-box bits of string pointers read as doubles
                                    // (closes #41).
                                    Expr::ProcessArgv
                        ) {
                            let mut args_iter = args.into_iter();
                            let start = args_iter.next().unwrap();
                            let end = args_iter.next();
                            return Ok(Ok(Expr::ArraySlice {
                                array: Box::new(array_expr),
                                start: Box::new(start),
                                end: end.map(Box::new),
                            }));
                        }
                        // Fall through to generic Call handling (could be a String.slice).
                    }
                    // .join() folds to Array.join only when the receiver is
                    // statically array-producing. Pre-fix the unconditional fold
                    // misrouted drizzle's `sql.join(arr)` (the `sql` template tag
                    // function has its own `.join` static method via
                    // `sql2.join = join`) to `js_array_join(sql, arr)`, which
                    // returned an empty string. Refs #420.
                    //
                    // Mirrors the "slice" arm above: fold only for array-producing
                    // shapes (literals, .map/.filter/.split chains, etc.). Other
                    // receivers fall through to generic dispatch which respects
                    // user-defined `.join` methods.
                    "join" if args.len() <= 1 && !recv_is_class => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        let is_array_producing = matches!(
                            &array_expr,
                            Expr::Array(_)
                                | Expr::ArrayMap { .. }
                                | Expr::ArrayFilter { .. }
                                | Expr::ArraySort { .. }
                                | Expr::ArraySlice { .. }
                                | Expr::ArraySpread(_)
                                | Expr::ArrayFrom(_)
                                | Expr::ArrayFromArrayLikeHoley(_)
                                | Expr::ArrayFromMapped { .. }
                                | Expr::ArrayFlat { .. }
                                | Expr::StringSplit(_, _)
                                | Expr::ArrayToReversed { .. }
                                | Expr::ArrayToSorted { .. }
                                | Expr::ArrayToSpliced { .. }
                                | Expr::ArrayWith { .. }
                                | Expr::ArrayEntries(_)
                                | Expr::ArrayKeys(_)
                                | Expr::ArrayValues(_)
                                | Expr::ObjectKeys(_)
                                | Expr::ObjectValues(_)
                                | Expr::ObjectEntries(_)
                                | Expr::ProcessArgv
                        );
                        // Also fold when the receiver is statically Array-typed
                        // via `lookup_local_type` (e.g. `const arr: string[] = ...`).
                        let is_array_local = if let ast::Expr::Ident(ident) = member.obj.as_ref() {
                            matches!(
                                ctx.lookup_local_type(ident.sym.as_ref()),
                                Some(Type::Array(_))
                            )
                        } else {
                            false
                        };
                        if is_array_producing || is_array_local {
                            let separator = if args.is_empty() {
                                None
                            } else {
                                Some(Box::new(args.into_iter().next().unwrap()))
                            };
                            return Ok(Ok(Expr::ArrayJoin {
                                array: Box::new(array_expr),
                                separator,
                            }));
                        }
                        // Fall through to generic dispatch — could be a user
                        // object's `.join` method (drizzle's sql.join, etc.).
                    }
                    "indexOf" if args.len() == 1 || args.len() == 2 => {
                        if recv_is_known_string_prop {
                            // Fall through to string/generic dispatch.
                        } else {
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            // #5195: a `PropertyGet` receiver
                            // (`arr[i].id.indexOf(...)`, `p.id.indexOf(...)`) is
                            // NOT necessarily an array — the property is very often
                            // a string. Folding it to `ArrayIndexOf` (an
                            // element-equality search) silently returned the wrong
                            // result for string-typed properties; the old hardcoded
                            // `stack|message|name|…` allowlist only patched a
                            // handful of property names. Only fold receivers that
                            // are *provably* array-producing; every other receiver
                            // (all property accesses included) falls through to the
                            // generic method dispatch, which checks the runtime
                            // value type and routes string- vs. array-`indexOf`
                            // correctly — exactly as the `slice` arm above does.
                            if matches!(
                                &array_expr,
                                Expr::ArrayMap { .. }
                                    | Expr::ArrayFilter { .. }
                                    | Expr::ArraySort { .. }
                                    | Expr::ArraySlice { .. }
                                    | Expr::Array(_)
                                    | Expr::ArrayFrom(_)
                                    | Expr::ArrayFromArrayLikeHoley(_)
                                    | Expr::StringSplit(_, _)
                                    | Expr::ObjectKeys(_)
                                    | Expr::ObjectValues(_)
                            ) {
                                let mut it = args.into_iter();
                                let value_expr = it.next().unwrap();
                                let from_index = it.next().map(Box::new);
                                return Ok(Ok(Expr::ArrayIndexOf {
                                    array: Box::new(array_expr),
                                    value: Box::new(value_expr),
                                    from_index,
                                }));
                            }
                        }
                    }
                    "includes" if args.len() == 1 || args.len() == 2 => {
                        if recv_is_known_string_prop {
                            // Fall through to string/generic dispatch.
                        } else {
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            // #5195: see the `indexOf` arm above — a `PropertyGet`
                            // receiver is not necessarily an array (the property is
                            // often a string), so folding it to `ArrayIncludes`
                            // silently returned `false` on string properties. Only
                            // fold provably array-producing receivers; property
                            // accesses fall through to the runtime-type-checked
                            // generic dispatch.
                            if matches!(
                                &array_expr,
                                Expr::ArrayMap { .. }
                                    | Expr::ArrayFilter { .. }
                                    | Expr::ArraySort { .. }
                                    | Expr::ArraySlice { .. }
                                    | Expr::Array(_)
                                    | Expr::ArrayFrom(_)
                                    | Expr::ArrayFromArrayLikeHoley(_)
                                    | Expr::StringSplit(_, _)
                                    | Expr::ObjectKeys(_)
                                    | Expr::ObjectValues(_)
                            ) {
                                let mut it = args.into_iter();
                                let value_expr = it.next().unwrap();
                                let from_index = it.next().map(Box::new);
                                return Ok(Ok(Expr::ArrayIncludes {
                                    array: Box::new(array_expr),
                                    value: Box::new(value_expr),
                                    from_index,
                                }));
                            }
                        }
                    }
                    "flat" if args.is_empty() => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayFlat {
                            array: Box::new(array_expr),
                        }));
                    }
                    "reduceRight" if !args.is_empty() => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
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
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArrayToReversed {
                            array: Box::new(array_expr),
                        }));
                    }
                    "toSorted" => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        let comparator = args.into_iter().next().map(Box::new);
                        return Ok(Ok(Expr::ArrayToSorted {
                            array: Box::new(array_expr),
                            comparator,
                        }));
                    }
                    "toSpliced" => {
                        // #2794: Node treats omitted args as valid. 0 args ->
                        // shallow copy (start 0, deleteCount 0); 1 arg -> delete
                        // from start through the end (deleteCount = +Infinity,
                        // clamped in the runtime); 2+ args -> explicit count.
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        let arg_count = args.len();
                        let mut args_iter = args.into_iter();
                        let start = args_iter.next().unwrap_or(Expr::Number(0.0));
                        let delete_count = match args_iter.next() {
                            Some(dc) => dc,
                            // 1 arg: delete through end; 0 args: delete nothing.
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
                    "with" if args.len() >= 2 => {
                        // Array.prototype.with(idx, value) — only fold when the
                        // receiver is statically known to be array-like. The
                        // `with` method name is heavily overloaded with user-defined
                        // builder methods (`class Builder { with(a, b): this { … } }`,
                        // `const obj = { with(a, b) { … } }`); aggressively folding
                        // non-array receivers silently rewrites the call to a
                        // typed-array index-replace and breaks user code.
                        //
                        // The sibling array arms (`map`/`filter`/`reduce`) fold
                        // optimistically because the runtime's `js_native_call_method`
                        // has dispatch arms for them — `with` has no such arm, but
                        // that's fine: bailing here falls through to the general
                        // method-call dispatch which finds the user's `with` method.
                        // The known-array fold paths for bare-ident receivers
                        // (line ~2897), imported-var array receivers (line ~3432),
                        // and inline array literals (line ~3610) handle the
                        // legitimate `Array.prototype.with` cases. (#515)
                        let recv_is_array_like = match member.obj.as_ref() {
                            ast::Expr::Ident(ident) => {
                                let n = ident.sym.to_string();
                                ctx.lookup_local_type(&n)
                                    .map(|ty| matches!(ty, Type::Array(_) | Type::Tuple(_)))
                                    .unwrap_or(false)
                            }
                            ast::Expr::Array(_) => true,
                            _ => false,
                        };
                        if recv_is_array_like {
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            let mut args_iter = args.into_iter();
                            let index = args_iter.next().unwrap();
                            let value = args_iter.next().unwrap();
                            return Ok(Ok(Expr::ArrayWith {
                                array: Box::new(array_expr),
                                index: Box::new(index),
                                value: Box::new(value),
                            }));
                        }
                        // Fall through to general method-call dispatch
                    }
                    "push" => {
                        // Generic expr.push(value) or expr.push(...spread)
                        // GUARD: Skip if the receiver is a user-defined class instance
                        // (e.g. Stack<T>.push()), or an object type literal (e.g.
                        // { push: (v) => void, ... }), so its method dispatches correctly.
                        let is_user_class_receiver = match member.obj.as_ref() {
                            ast::Expr::This(_) => true,
                            ast::Expr::Ident(ident) => {
                                ctx.lookup_local_type(ident.sym.as_ref())
                                    .map(|ty| {
                                        match ty {
                                            // A class instance OR an interface-typed value is the
                                            // receiver's OWN object and may own a `push` method, so
                                            // never fold to the array intrinsic. Interfaces aren't
                                            // classes (`lookup_class` misses them), so the previous
                                            // `lookup_class(name).is_some()` folded an interface
                                            // receiver's `push` to the array fast path — reading the
                                            // object header as an ArrayHeader and dropping the call
                                            // (follow-up to #5139, which fixed only `any` receivers).
                                            Type::Named(name) => {
                                                ctx.lookup_class(name).is_some()
                                                    || ctx.is_interface_type(name)
                                            }
                                            Type::Generic { base, .. } => {
                                                let builtin =
                                                    ["Map", "Set", "WeakMap", "WeakSet", "Promise"];
                                                !builtin.contains(&base.as_str())
                                                    && ctx.lookup_class(base).is_some()
                                            }
                                            Type::Object(_) => true, // object type literal with push property
                                            _ => false,
                                        }
                                    })
                                    .unwrap_or(false)
                            }
                            ast::Expr::New(_) => true, // new ClassName().push()
                            _ => false,
                        };
                        // wall 49: `recv_is_class` is true for an unknown/`Any`
                        // receiver (e.g. `const m = new mod.Class(); m.push(x)`,
                        // where the cross-module dynamic `new` leaves `m` typed
                        // `Any`). Folding to a native array push corrupts the
                        // class instance — its ObjectHeader is reinterpreted as an
                        // ArrayHeader and the user `push` method never runs. Bail
                        // to dynamic dispatch, which resolves the class method
                        // first and falls back to the array op for real arrays.
                        if !is_user_class_receiver && !recv_is_class {
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            let any_spread = call.args.iter().any(|a| a.spread.is_some());
                            if !any_spread {
                                // No spreads — `push_single` loops over every
                                // arg, so a single native call handles N values.
                                return Ok(Ok(Expr::NativeMethodCall {
                                    module: "array".to_string(),
                                    method: "push_single".to_string(),
                                    class_name: None,
                                    object: Some(Box::new(array_expr)),
                                    args,
                                }));
                            }
                            if args.len() == 1 {
                                // Exactly one spread arg — the `push_spread`
                                // native arm packs the source as `args[0]`.
                                return Ok(Ok(Expr::NativeMethodCall {
                                    module: "array".to_string(),
                                    method: "push_spread".to_string(),
                                    class_name: None,
                                    object: Some(Box::new(array_expr)),
                                    args,
                                }));
                            }
                            // Mixed/multiple args with at least one spread
                            // (e.g. `arr.push(...a, ...b)` or `arr.push(...a, x)`
                            // inside a method/getter body). The single-arg
                            // `push_spread`/`push_single` native arms each
                            // expect exactly one arg; the previous code passed
                            // all of them to one `push_spread`, which bailed at
                            // codegen ("expects exactly 1 arg"). Decompose into a
                            // `Sequence` of single-arg native calls, choosing the
                            // helper per-arg from the original AST spread flag.
                            // Each native arm re-reads the receiver and writes
                            // back the (possibly realloc'd) handle, so chaining
                            // threads the array correctly; JS `push` returns the
                            // final length, which is exactly what the last
                            // element of the `Sequence` yields. (#4508)
                            let mut stmts: Vec<Expr> = Vec::with_capacity(args.len());
                            for (ast_arg, arg) in call.args.iter().zip(args) {
                                let method = if ast_arg.spread.is_some() {
                                    "push_spread"
                                } else {
                                    "push_single"
                                };
                                stmts.push(Expr::NativeMethodCall {
                                    module: "array".to_string(),
                                    method: method.to_string(),
                                    class_name: None,
                                    object: Some(Box::new(array_expr.clone())),
                                    args: vec![arg],
                                });
                            }
                            return Ok(Ok(Expr::Sequence(stmts)));
                        }
                    }
                    _ => {} // Fall through - ambiguous methods on non-array expressions use generic dispatch
                }
            }
        }
    }

    Ok(Err(args))
}
