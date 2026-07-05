//! Class-receiver / promise / array static-type predicates + `static_type_of`.
//!
//! Split out of `type_analysis.rs` (file-size gate). Pure code move.

use super::*;

use perry_hir::{BinaryOp, Expr, UnaryOp};
use perry_types::Type as HirType;

use crate::expr::FnCtx;
use crate::type_analysis_class_fields::{
    class_field_declared_type, class_field_global_index, declared_field_type,
};
use crate::type_analysis_facts::{
    function_type_from_decl, hir_inferred_refinable_type, hir_inferred_static_type,
};
use crate::type_analysis_net::{net_result_class, net_result_type};

/// Statically determine whether an expression evaluates to a Promise.
/// #1008: does `expr` refer to a built-in global (e.g. `Promise`,
/// `Array`)? Recognises both shapes that the HIR lowers bare global
/// idents into:
///
/// - Legacy: `Expr::GlobalGet(_)` directly. Pre-#973 codepath.
/// - Post-#973: `Expr::PropertyGet { object: GlobalGet(0), property:
///   <name> }`. After PR #973, bare built-in idents lower as a
///   property access on `globalThis` so they route through the
///   globalThis singleton closure path. Old call sites that only
///   matched the legacy shape silently lost specialization.
///
/// Pass `name = "Promise"` (etc.) to require the property-access form
/// to actually name that built-in; the legacy `GlobalGet(_)` arm
/// accepts any global because the original code never narrowed.
// `dead_code` allow: the function survived an unresolved merge in
// main (commit 9a9a233c's "fix: recognize global Promise static
// calls" left HEAD/incoming markers in this file). The
// `is_global_constructor_expr` helper added by the same commit
// supersedes this one, but ripping it out is outside #516's
// scope — leave the lingering definition with an allow so the
// dead-code lint doesn't fail the build.
#[allow(dead_code)]
pub(crate) fn is_global_builtin_named(expr: &Expr, name: &str) -> bool {
    if matches!(expr, Expr::GlobalGet(_)) {
        return true;
    }
    if let Expr::PropertyGet { object, property } = expr {
        if matches!(object.as_ref(), Expr::GlobalGet(_)) && property == name {
            return true;
        }
    }
    false
}

/// Used by `.then()` / `.catch()` / `.finally()` dispatch in lower_call
/// to intercept promise method calls and route them through the runtime
/// `js_promise_then` / `js_promise_catch` functions.
///
/// Recognizes:
/// - LocalGet of a `Promise(_)`-typed local
/// - `Promise.resolve(x)` / `Promise.reject(x)` / `Promise.all(x)` / etc.
///   (the GlobalGet + "resolve"/"reject"/"all"/"race"/"allSettled" pattern)
/// - Result of `.then(cb)` / `.catch(cb)` / `.finally(cb)` on a promise
///   (recursive: chains like `p.then(f).then(g)`)
/// - Async function calls (return type is Promise)
pub(crate) fn is_promise_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id) {
            Some(HirType::Promise(_)) => true,
            // `const p: Promise<T> = ...` is lowered as Generic { base: "Promise", ... }
            // by the HIR when the source annotation is `Promise<T>` rather than the
            // async-function return inference path (which produces HirType::Promise).
            Some(HirType::Generic { base, .. }) if base == "Promise" => true,
            _ => false,
        },
        // Promise.resolve / reject / all / race / allSettled / any
        Expr::Call { callee, .. } => match callee.as_ref() {
            Expr::PropertyGet { object, property } => {
                // `Promise.resolve(...)` etc. The receiver `Promise` can
                // appear in two shapes:
                //   - Legacy: bare ident → `Expr::GlobalGet(_)` directly.
                //   - Post-#973: bare built-in idents lower to
                //     `PropertyGet { GlobalGet(0), "Promise" }` so they
                //     route through the globalThis singleton closure
                //     path. Without the second arm, `is_promise_expr`
                //     returned false for `Promise.resolve()` and the
                //     `.then` codegen fell through to generic native
                //     dispatch — microtask-02..07 and edge-promises went
                //     silent (callbacks never enqueued). (#1008)
                //
                // Resolved-from-merge note: the HEAD side called
                // `is_global_builtin_named`, the incoming side called
                // `is_global_constructor_expr`. Post-#1030 the rest of
                // the codegen prefers the latter helper, so we keep the
                // richer HEAD comment but switch to the canonical call.
                if matches!(
                    property.as_str(),
                    "resolve" | "reject" | "all" | "race" | "allSettled" | "any"
                ) && is_global_builtin_named(object.as_ref(), "Promise")
                {
                    return true;
                }
                // `Array.fromAsync(...)` returns a Promise<Array>.
                if property == "fromAsync" && is_global_builtin_named(object.as_ref(), "Array") {
                    return true;
                }
                // `.then(cb)` / `.catch(cb)` / `.finally(cb)` on a promise
                // receiver — the result is itself a promise.
                if matches!(property.as_str(), "then" | "catch" | "finally")
                    && is_promise_expr(ctx, object)
                {
                    return true;
                }
                // Issue #489 followup: `obj.field(args)` where `field` is
                // typed as an async function or a function returning
                // `Promise<T>`. Drizzle's `mysql-proxy/session.js` calls
                // `this.client(...).then(({rows}) => rows)` where
                // `this.client` is a class field of type
                // `(sql, params, method) => Promise<{rows, …}>`. Without
                // this arm, perry's `.then` lowering doesn't recognize
                // the call result as a Promise and falls through to a
                // generic dispatch that silently drops the callback (the
                // await of `db.insert(...)` resolves to undefined / "").
                if let Some(HirType::Function(ft)) = static_type_of(ctx, callee.as_ref()) {
                    if ft.is_async {
                        return true;
                    }
                    if matches!(*ft.return_type, HirType::Promise(_)) {
                        return true;
                    }
                    if let HirType::Generic { ref base, .. } = *ft.return_type {
                        if base == "Promise" {
                            return true;
                        }
                    }
                }
                // Issue #489 followup: `obj.method(args)` where `method`
                // is a class instance method declared `async` or with a
                // return type of `Promise<T>`. Class methods live in
                // `class.methods` (not `class.fields`), so the
                // static_type_of branch above doesn't catch them. Walk
                // the parent chain for inherited async methods too —
                // drizzle's `MySqlInsertBase.execute` is a class-field
                // arrow defined on the subclass, but the override-vs-
                // inherited shape varies per query-builder, so handle
                // both. The fallback class_name comes from the receiver.
                if let Some(class_name) = receiver_class_name(ctx, object) {
                    let mut current = Some(class_name);
                    while let Some(cn) = current {
                        if let Some(class) = ctx.classes.get(&cn) {
                            if let Some(m) = class.methods.iter().find(|m| m.name == *property) {
                                if m.is_async {
                                    return true;
                                }
                                match &m.return_type {
                                    HirType::Promise(_) => return true,
                                    HirType::Generic { base, .. } if base == "Promise" => {
                                        return true
                                    }
                                    _ => {}
                                }
                            }
                            current = class.extends_name.clone();
                        } else {
                            break;
                        }
                    }
                }
                false
            }
            // Direct call to a locally-defined async function — its
            // return value is a `Promise<T>`. The HIR's
            // `Function::is_async` flag is collected into
            // `cross_module.local_async_funcs` at module compile time.
            Expr::FuncRef(fid) => ctx.local_async_funcs.contains(fid),
            // Issue #633 / #611 followup: call to a local LET-bound
            // async closure — `const fn = async (...) => ...; fn(...)`.
            // The let's type is `HirType::Function { is_async: true }`,
            // recorded in `local_types`. Without this arm, perry's
            // `.then()` lowering at `lower_call.rs:1188` doesn't
            // recognize `fn({}).then(cb)` as a Promise receiver and the
            // .then call falls through to a generic dispatch that
            // silently drops the callback.
            Expr::LocalGet(id) => match ctx.local_types.get(id) {
                Some(HirType::Function(ft)) if ft.is_async => true,
                Some(HirType::Function(ft)) => match ft.return_type.as_ref() {
                    HirType::Promise(_) => true,
                    HirType::Generic { base, .. } if base == "Promise" => true,
                    _ => false,
                },
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}

/// If the expression is a known instance of a Named class type, return
/// the class name. Used by the class method dispatch in lower_call to
/// pick the right `perry_method_<class>_<name>` function.
pub(crate) fn receiver_class_name(ctx: &FnCtx<'_>, e: &Expr) -> Option<String> {
    match e {
        Expr::LocalGet(id) => match ctx.local_types.get(id)? {
            HirType::Named(name) => Some(name.clone()),
            // Generic instantiation `SimpleContainer<number>`: prefer the
            // MONOMORPHIZED specialization `base$mangled` whenever it is
            // registered. The instance is genuinely a `SimpleContainer$num`
            // (concrete field types), and — critically — the escape analysis
            // that authorizes scalar replacement keys off the `new`'s
            // specialized class name (collect_non_escaping_news). If codegen
            // resolved to the base `SimpleContainer` instead (whose fields are
            // still `T`), the scalar-method summary would reject `get()` and a
            // scalar-replaced receiver would fall through to normal dispatch on
            // an uninitialized dummy slot (#6040: `(number).get is not a
            // function`). Resolving to the specialization keeps the two passes
            // consistent. Fall back to the base template when no specialization
            // exists (fully-generic code paths), then give up.
            HirType::Generic { base, type_args } => {
                let specialized = perry_hir::monomorph::generate_specialized_name(base, type_args);
                if ctx.classes.contains_key(&specialized) {
                    Some(specialized)
                } else if ctx.classes.contains_key(base) {
                    Some(base.clone())
                } else {
                    None
                }
            }
            _ => None,
        },
        // `new ClassName(...)` — the receiver class is the constructed class.
        // Lets `(new Config()).toString()` find Config's user toString.
        Expr::New { class_name, .. } => Some(class_name.clone()),
        // `ClassName.staticMethod(...)` chains often return an instance
        // of `ClassName` (factory pattern: `Color.red()`). Without type
        // info on the static method's return, assume it's the same class
        // so chained `.toString()` finds the user's toString.
        Expr::StaticMethodCall { class_name, .. } => Some(class_name.clone()),
        e if net_result_class(e).is_some() => net_result_class(e).map(str::to_string),
        // `this` inside a constructor or method body — the class name is
        // at the top of class_stack (for inlined constructors) or comes
        // from the enclosing method's owning class.
        Expr::This => ctx.class_stack.last().cloned(),
        // A private-access brand guard returns its receiver unchanged; see
        // through it so shadowed private-field slot resolution stays accurate.
        Expr::PrivateGuard { object, .. } => receiver_class_name(ctx, object),
        // `arr[i]` where `arr: ClassFoo[]` — the element type is the
        // array's parameter. Lets `items[2].display()` resolve the
        // method dispatch.
        Expr::IndexGet { object, .. } => {
            if let Expr::LocalGet(arr_id) = object.as_ref() {
                if let Some(HirType::Array(elem)) = ctx.local_types.get(arr_id) {
                    if let HirType::Named(name) = elem.as_ref() {
                        return Some(name.clone());
                    }
                }
            }
            None
        }
        // `this.field` or `obj.field` where the field's declared type
        // is a class. Walk the class definition to find the field's
        // type. Honors the parent inheritance chain.
        Expr::PropertyGet { object, property } => {
            let owner_class_name = receiver_class_name(ctx, object)?;
            let class = ctx.classes.get(&owner_class_name)?;
            // Look in own fields, then walk parent chain.
            let field_ty = class
                .fields
                .iter()
                .find(|f| f.name == *property)
                .map(|f| &f.ty)
                .or_else(|| {
                    let mut parent = class.extends_name.as_deref();
                    while let Some(p) = parent {
                        if let Some(pc) = ctx.classes.get(p) {
                            if let Some(f) = pc.fields.iter().find(|f| f.name == *property) {
                                return Some(&f.ty);
                            }
                            parent = pc.extends_name.as_deref();
                        } else {
                            break;
                        }
                    }
                    None
                })?;
            match field_ty {
                HirType::Named(name) => Some(name.clone()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Statically determine whether an expression is an array. Used for
/// dispatch on `arr.length` and `arr[i]`.
///
/// Recognizes:
/// - literal arrays `[a, b, c]` and `Expr::ArraySpread`
/// - LocalGet of an Array-typed local
/// - **PropertyGet on a class instance where the field is Array-typed**
///   (e.g. `this.items` when `Container.items: Item[]`)
/// - **NativeMethodCall results where the runtime returns an array**
///   (e.g. `arr.map(...)` — but those use the special Expr::ArrayMap
///   variant which is already handled)
pub(crate) fn is_array_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match static_type_of(ctx, e) {
        Some(HirType::Array(_)) | Some(HirType::Tuple(_)) => true,
        Some(HirType::Generic { ref base, .. }) if base == "Array" => true,
        // #3148: %TypedArray% receivers route their not-already-folded methods
        // (fill / reverse / keys / values / entries / set / subarray) through
        // `lower_array_method`; the generic `js_array_*` helpers delegate to the
        // element-typed `js_typed_array_*` impls via `lookup_typed_array_kind`.
        // Uint8Array / Uint8ClampedArray are intentionally excluded — they are
        // buffer-backed and dispatched by `dispatch_buffer_method`.
        Some(HirType::Named(ref n))
            if matches!(
                n.as_str(),
                "Int8Array"
                    | "Int16Array"
                    | "Int32Array"
                    | "Uint16Array"
                    | "Uint32Array"
                    | "Float16Array"
                    | "Float32Array"
                    | "Float64Array"
                    | "BigInt64Array"
                    | "BigUint64Array"
            ) =>
        {
            true
        }
        // `T | null`, `T | undefined`, `T[] | null` — when an `if (x)`
        // guard narrows away the null/undefined, the truthy branch
        // still has the same union type in the HIR, so recognize
        // unions whose non-nullish variant is an array. Without this
        // `maybeArr.length` falls through to object-field access and
        // prints `undefined`.
        Some(HirType::Union(variants)) => variants
            .iter()
            .any(|v| matches!(v, HirType::Array(_) | HirType::Tuple(_))),
        _ => false,
    }
}

/// True when `e` is a *dynamic* index into a native-module namespace —
/// the auditable `ns[dynamicKey]` sub-namespace-selection shape (#1740,
/// e.g. `(path as any)[k]` resolving to `path.win32` / `path.posix`).
///
/// Such a receiver evaluates to a native-module sub-object at runtime,
/// never a primitive, so a method call on it must route through the
/// generic `js_native_call_method` dispatch (which reaches
/// `dispatch_native_module_method`) rather than being mis-classified as
/// a `String`/`Number` prototype method by its name alone. Without this,
/// a prototype-colliding name like `normalize` is lowered as a string
/// method and the namespace pointer is handed to a string FFI → SIGSEGV
/// (#1760).
///
/// Gated on a *non-literal* index: `(path as any)["sep"]` (a literal
/// string property) can legitimately resolve to a string and must keep
/// its string-method lowering, whereas `(path as any)[k]` is the dynamic
/// sub-namespace form this targets.
pub(crate) fn is_native_module_dynamic_index(e: &Expr) -> bool {
    matches!(
        e,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::NativeModuleRef(_))
                && !matches!(index.as_ref(), Expr::String(_) | Expr::WtfString(_))
    )
}

/// Best-effort static type lookup for an expression. Returns the HIR
/// type when it's cheap to determine (literals, locals, field accesses
/// on known classes). Returns `None` when computing the type would
/// require a fuller type-checker pass.
/// Extract a non-negative integer literal index from an index expression, if it
/// is one. Used to type tuple element accesses only for in-bounds literal
/// indices (dynamic indices into a heterogeneous tuple aren't statically known).
pub(crate) fn tuple_index_literal(index: &Expr) -> Option<usize> {
    match index {
        Expr::Integer(n) if *n >= 0 => Some(*n as usize),
        Expr::Number(f) if *f >= 0.0 && f.fract() == 0.0 => Some(*f as usize),
        _ => None,
    }
}

pub(crate) fn static_type_of(ctx: &FnCtx<'_>, e: &Expr) -> Option<HirType> {
    match e {
        Expr::Array(_) => Some(HirType::Array(Box::new(HirType::Any))),
        Expr::String(_) | Expr::WtfString(_) => Some(HirType::String),
        Expr::Number(_) | Expr::Integer(_) => Some(HirType::Number),
        Expr::Bool(_) => Some(HirType::Boolean),
        Expr::LocalGet(id) => ctx.local_types.get(id).cloned(),
        Expr::StaticMethodCall {
            class_name,
            method_name,
            ..
        } => ctx
            .classes
            .get(class_name)
            .and_then(|class| {
                class
                    .static_methods
                    .iter()
                    .find(|method| method.name == *method_name)
            })
            .map(|method| method.return_type.clone()),
        e if net_result_type(e).is_some() => net_result_type(e),
        Expr::PropertyGet { object, property } => {
            if property == "length" && expression_has_numeric_length(ctx, object) {
                return Some(HirType::Number);
            }
            if pod_record_field_is_numeric(ctx, object, property) {
                return Some(HirType::Number);
            }
            if is_process_namespace_version_property(object, property) {
                return Some(HirType::String);
            }
            if matches!(property.as_str(), "publicKey" | "privateKey")
                && matches!(
                    static_type_of(ctx, object),
                    Some(HirType::Named(ref name)) if name == "CryptoKeyPair"
                )
            {
                return Some(HirType::String);
            }
            if let Some(static_method_ty) = crate::expr::try_static_class_name(object, ctx)
                .and_then(|class_name| ctx.classes.get(class_name))
                .and_then(|class| {
                    class
                        .static_methods
                        .iter()
                        .find(|method| method.name == *property)
                        .map(function_type_from_decl)
                })
            {
                return Some(static_method_ty);
            }
            if let Some(receiver_class) = receiver_class_name(ctx, object) {
                // If the object is a known class instance, look up the field
                // type from the class definition.
                if let Some(class) = ctx.classes.get(&receiver_class) {
                    if let Some(field_ty) = class
                        .fields
                        .iter()
                        .find(|f| f.name == *property)
                        .map(|f| f.ty.clone())
                        .or_else(|| {
                            // Walk up the inheritance chain.
                            let mut parent = class.extends_name.as_deref();
                            while let Some(p) = parent {
                                if let Some(pc) = ctx.classes.get(p) {
                                    if let Some(field) =
                                        pc.fields.iter().find(|f| f.name == *property)
                                    {
                                        return Some(field.ty.clone());
                                    }
                                    parent = pc.extends_name.as_deref();
                                } else {
                                    break;
                                }
                            }
                            None
                        })
                    {
                        return Some(field_ty);
                    }
                    if let Some(method_ty) = class
                        .methods
                        .iter()
                        .find(|method| method.name == *property)
                        .map(function_type_from_decl)
                    {
                        return Some(method_ty);
                    }
                }
                // Issue #655: receiver may be typed against a TS `interface`
                // rather than a class. The runtime layout is identical to a
                // plain object literal, so the property's declared type is
                // the right answer for the array fast-path / `length=` setter
                // path. Walks the `extends` chain too so chained interfaces
                // (`interface Sub extends Base { ... }`) resolve.
                if let Some(iface) = ctx.interfaces.get(&receiver_class) {
                    if let Some(p) = iface.properties.iter().find(|p| p.name == *property) {
                        return Some(p.ty.clone());
                    }
                    if let Some(method) =
                        iface.methods.iter().find(|method| method.name == *property)
                    {
                        return Some(HirType::Function(perry_types::FunctionType {
                            params: method.params.clone(),
                            return_type: Box::new(method.return_type.clone()),
                            is_async: false,
                            is_generator: false,
                        }));
                    }
                    for ext in &iface.extends {
                        if let HirType::Named(parent_name) = ext {
                            if let Some(parent_iface) = ctx.interfaces.get(parent_name) {
                                if let Some(p) =
                                    parent_iface.properties.iter().find(|p| p.name == *property)
                                {
                                    return Some(p.ty.clone());
                                }
                            }
                        }
                    }
                }
            }
            hir_inferred_static_type(ctx, e)
        }
        Expr::This => {
            let cls = ctx.class_stack.last()?.clone();
            Some(HirType::Named(cls))
        }
        // `str.split(delim)` returns Array<String>. Catches the generic
        // Call form that bypasses the `Expr::StringSplit` variant — e.g.
        // `"a,b,c".split(",")` in an expression position where we need
        // `.length` / `[i]` to follow the array fast path.
        // Also: `str.match(regex)` produces an array. `matchAll` deliberately
        // stays dynamic because it returns a RegExp String Iterator object.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object } if matches!(
                    property.as_str(), "split" | "match"
                ) && is_string_expr(ctx, object)
            ) =>
        {
            Some(HirType::Array(Box::new(HirType::String)))
        }
        // `crypto.createHash(alg).update(d).digest()` with no encoding arg
        // returns a Buffer. Recognizing the inline chain (not just a bound
        // local) lets `...digest().toString('hex')` / `...digest()[i]` take
        // the buffer dispatch instead of the Latin-1 string path (#1353).
        Expr::Call { callee, args, .. }
            if args.first().is_none_or(|a| matches!(a, Expr::Undefined))
                && is_crypto_digest_chain(callee) =>
        {
            Some(HirType::Named("Uint8Array".into()))
        }
        // crypto.getHashes()/getCiphers()/getCurves() all return
        // Array<string>. Recognize this even in expression position so
        // chained `.includes(...)` uses Array SameValueZero instead of
        // falling through to dynamic/string dispatch.
        Expr::Call { callee, .. }
            if matches!(
                callee.as_ref(),
                Expr::PropertyGet { property, object }
                    if matches!(object.as_ref(), Expr::NativeModuleRef(m) if m == "crypto")
                        && matches!(property.as_str(), "getHashes" | "getCiphers" | "getCurves")
            ) =>
        {
            Some(HirType::Array(Box::new(HirType::String)))
        }
        Expr::Call { callee, .. } => {
            if let Some(HirType::Function(ft)) = static_type_of(ctx, callee.as_ref()) {
                return Some((*ft.return_type).clone());
            }
            hir_inferred_static_type(ctx, e)
        }
        // `arr[i]` where `arr: Array<T>` has static type `T`. This lets
        // nested access like `grid[i][j]` and `grid[i].length` reach
        // the array fast paths (via is_array_expr) when `grid` is
        // statically known to be `Array<Array<T>>` / `Array<Tuple<...>>`.
        // Also handles `Record<K, V>[key]` → V so `groups["a"].length`
        // on `Record<string, number[]>` finds the array fast path.
        Expr::IndexGet { object, index } => match static_type_of(ctx, object) {
            Some(HirType::Array(inner)) => Some(*inner),
            // A literal, in-bounds index has the exact element type. A dynamic
            // index could hit any element, so it's only sound when the tuple is
            // homogeneous — otherwise stay conservative (e.g. `[string, number]`
            // must not type `t[i]` as `string`).
            Some(HirType::Tuple(elems)) if !elems.is_empty() => match tuple_index_literal(index) {
                Some(i) => elems.get(i).cloned(),
                None => {
                    let first = &elems[0];
                    elems.iter().all(|t| t == first).then(|| first.clone())
                }
            },
            Some(HirType::Generic { base, type_args })
                if base == "Record" && type_args.len() == 2 =>
            {
                Some(type_args[1].clone())
            }
            _ => hir_inferred_static_type(ctx, e),
        },
        // `a || b` and `a ?? b` lower to `Expr::Logical`. Recognize the
        // result as Array-typed when EITHER branch is Array — `is_array_expr`
        // already accepts the Union form, so this lets `(maybeArr || []).slice()`
        // route through the array fast path instead of falling through to
        // `js_native_call_method`, which has no `slice` arm for arrays and
        // returns a sentinel that downstream `.sort(cmp)` deref's to null
        // (issue #291). `&&` likewise — its truthy result is the right
        // operand which is an array literal in the common idiom.
        Expr::Logical { left, right, .. } => {
            let lt = static_type_of(ctx, left);
            let rt = static_type_of(ctx, right);
            match (lt, rt) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(a), Some(b)) => Some(HirType::Union(vec![a, b])),
                (Some(t), None) | (None, Some(t)) => Some(t),
                _ => None,
            }
        }
        // `cond ? a : b` — same logic as Logical.
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let lt = static_type_of(ctx, then_expr);
            let rt = static_type_of(ctx, else_expr);
            match (lt, rt) {
                (Some(a), Some(b)) if a == b => Some(a),
                (Some(a), Some(b)) => Some(HirType::Union(vec![a, b])),
                (Some(t), None) | (None, Some(t)) => Some(t),
                _ => None,
            }
        }
        _ => hir_inferred_static_type(ctx, e),
    }
}
