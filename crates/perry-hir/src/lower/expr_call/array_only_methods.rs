//! Array-only method dispatch on arbitrary expressions (Object.entries(x).reduce(...) etc.).
//!
//! Extracted from `expr_call/mod.rs` as a mechanical move.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_ecma_ast as ast;

use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

/// Is `expr` a reference to a node:stream class constructor — bare
/// (`Readable`) or namespaced (`stream.Readable`)? Used by
/// [`chain_roots_at_stream`].
fn is_stream_class_ref(expr: &ast::Expr) -> bool {
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

/// Does this expression's method chain originate from a node:stream
/// source — `Readable.from(...)` / `Readable.of(...)`, `new Transform()`,
/// or a chain of lazy iterator helpers (`map`/`filter`/`flatMap`/`take`/
/// `drop`) on top of one? (#1558)
///
/// The lazy stream helpers return another Readable, not an array, so a
/// chain like `Readable.from(x).map(f).filter(g)` must NOT be folded
/// into `Expr::Array<Method>` ops — `js_array_map` would read garbage
/// out of the stream object's header. Detecting the stream root here
/// keeps such chains on dynamic dispatch so the runtime's stream
/// iterator-helper stubs run. AST-only (no type info) so it catches the
/// common inline-chain form without depending on receiver inference.
fn chain_roots_at_stream(expr: &ast::Expr) -> bool {
    match expr {
        ast::Expr::Paren(p) => chain_roots_at_stream(&p.expr),
        ast::Expr::Await(a) => chain_roots_at_stream(&a.arg),
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
                "map" | "filter" | "flatMap" | "take" | "drop" => chain_roots_at_stream(&m.obj),
                _ => false,
            }
        }
        _ => false,
    }
}

use super::super::{
    extract_typed_parse_source_order, is_generator_call_expr, is_widget_modifier_name, lower_expr,
    resolve_typed_parse_ty, LoweringContext,
};

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
                // Helper: skip array-method dispatch when the receiver is a
                // known class instance (e.g. mongo `Collection.find`,
                // `Stack<T>.map`). Without this guard the lowering blindly
                // emits `Expr::Array<Method>` and the compiled binary calls
                // `js_array_<method>` on a class handle.
                let recv_is_class = match member.obj.as_ref() {
                    ast::Expr::Ident(ident) => {
                        let n = ident.sym.to_string();
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
                            let is_fetch_headers = prop_ident.sym.as_ref() == "headers"
                                && matches!(
                                    ctx.lookup_native_instance(obj_ident.sym.as_ref()),
                                    Some(("fetch", _))
                                        | Some(("Request", _))
                                        | Some(("Headers", _))
                                );
                            if is_fetch_headers {
                                true
                            } else {
                                false
                            }
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
                            // Look up the inner call's method name. If it's
                            // one of the known array-producing builtins, the
                            // chained fold IS safe — keep the ident-receiver
                            // optimistic behaviour for `arr.filter(p).find(q)`
                            // shapes.
                            let inner_method: Option<&str> = match &inner_call.callee {
                                ast::Callee::Expr(e) => match e.as_ref() {
                                    ast::Expr::Member(m) => match &m.prop {
                                        ast::MemberProp::Ident(i) => Some(i.sym.as_ref()),
                                        _ => None,
                                    },
                                    _ => None,
                                },
                                _ => None,
                            };
                            let inner_returns_array = inner_method
                                .map(|m| {
                                    matches!(
                                        m,
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
                                    )
                                })
                                .unwrap_or(false);
                            // recv_is_class = true means BAIL. Bail when the
                            // inner call is NOT a known array-producing method.
                            !inner_returns_array
                        }
                    }
                    _ => false,
                };
                // #1558: a node:stream chain (`Readable.from(x).map(f).filter(g)`)
                // must not be folded into Array<Method> ops — the lazy stream
                // transforms return a Readable, not an array, so `js_array_map`
                // would read garbage out of the stream object's header. Bail to
                // dynamic dispatch so the runtime's iterator-helper stubs run.
                let recv_is_class = recv_is_class || chain_roots_at_stream(member.obj.as_ref());
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
                    "entries" if args.is_empty() && !recv_is_class => {
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
                    "sort" if !args.is_empty() => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        return Ok(Ok(Expr::ArraySort {
                            array: Box::new(array_expr),
                            comparator: Box::new(args.into_iter().next().unwrap()),
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
                                    Expr::ArrayFrom(_) | Expr::ArrayFromMapped { .. } |
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
                    "indexOf" if !args.is_empty() => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        if matches!(
                            &array_expr,
                            Expr::ArrayMap { .. }
                                | Expr::ArrayFilter { .. }
                                | Expr::ArraySort { .. }
                                | Expr::ArraySlice { .. }
                                | Expr::Array(_)
                                | Expr::ArrayFrom(_)
                                | Expr::StringSplit(_, _)
                                | Expr::ObjectKeys(_)
                                | Expr::ObjectValues(_)
                                | Expr::PropertyGet { .. }
                        ) {
                            let value_expr = args.into_iter().next().unwrap();
                            return Ok(Ok(Expr::ArrayIndexOf {
                                array: Box::new(array_expr),
                                value: Box::new(value_expr),
                            }));
                        }
                    }
                    "includes" if !args.is_empty() => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        // Don't treat error string properties as arrays
                        let is_error_string_prop = matches!(&array_expr,
                            Expr::PropertyGet { property, .. }
                            if matches!(property.as_str(), "stack" | "message" | "name")
                        );
                        if !is_error_string_prop
                            && matches!(
                                &array_expr,
                                Expr::ArrayMap { .. }
                                    | Expr::ArrayFilter { .. }
                                    | Expr::ArraySort { .. }
                                    | Expr::ArraySlice { .. }
                                    | Expr::Array(_)
                                    | Expr::ArrayFrom(_)
                                    | Expr::StringSplit(_, _)
                                    | Expr::ObjectKeys(_)
                                    | Expr::ObjectValues(_)
                                    | Expr::PropertyGet { .. }
                            )
                        {
                            let value_expr = args.into_iter().next().unwrap();
                            return Ok(Ok(Expr::ArrayIncludes {
                                array: Box::new(array_expr),
                                value: Box::new(value_expr),
                            }));
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
                    "toSpliced" if args.len() >= 2 => {
                        let array_expr = lower_expr(ctx, &member.obj)?;
                        let mut args_iter = args.into_iter();
                        let start = args_iter.next().unwrap();
                        let delete_count = args_iter.next().unwrap();
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
                    "push" if !args.is_empty() => {
                        // Generic expr.push(value) or expr.push(...spread)
                        // GUARD: Skip if the receiver is a user-defined class instance
                        // (e.g. Stack<T>.push()), or an object type literal (e.g.
                        // { push: (v) => void, ... }), so its method dispatches correctly.
                        let is_user_class_receiver = match member.obj.as_ref() {
                            ast::Expr::Ident(ident) => {
                                ctx.lookup_local_type(ident.sym.as_ref())
                                    .map(|ty| {
                                        match ty {
                                            Type::Named(name) => ctx.lookup_class(name).is_some(),
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
                        if !is_user_class_receiver {
                            let array_expr = lower_expr(ctx, &member.obj)?;
                            if !call.args.is_empty() && call.args[0].spread.is_some() {
                                return Ok(Ok(Expr::NativeMethodCall {
                                    module: "array".to_string(),
                                    method: "push_spread".to_string(),
                                    class_name: None,
                                    object: Some(Box::new(array_expr)),
                                    args,
                                }));
                            } else {
                                return Ok(Ok(Expr::NativeMethodCall {
                                    module: "array".to_string(),
                                    method: "push_single".to_string(),
                                    class_name: None,
                                    object: Some(Box::new(array_expr)),
                                    args,
                                }));
                            }
                        }
                    }
                    _ => {} // Fall through - ambiguous methods on non-array expressions use generic dispatch
                }
            }
        }
    }

    Ok(Err(args))
}
