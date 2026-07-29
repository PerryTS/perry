//! `require(...)` namespace binding plus fetch/Web-Streams/Blob native-instance
//! registration for a simple `let/const/var` identifier binding (extracted from
//! `var_decl.rs`'s `Pat::Ident` arm).

use super::*;

use crate::types::Type;
use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

use crate::destructuring::helpers::{get_fetch_module, is_member_fetch_call};

/// Handles the `require(...)` namespace-binding fast path and the fetch /
/// Web-Streams / Blob native-instance registrations. May refine `ty`.
///
/// Returns `true` when the caller must early-return `Ok(result)` (the
/// `require`-of-a-resolvable-native-module case binds nothing observable).
/// Mirrors the original inline blocks verbatim.
pub(crate) fn register_native_fetch_and_streams(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    name: &str,
    ty: &mut Type,
) -> bool {
    // #5216: `const <name> = require("<spec>")` of a statically
    // resolvable native/Node-builtin module lowers to the same
    // module-namespace binding `import * as <name> from "<spec>"`
    // produces (native module + builtin alias, NO runtime `let` — a
    // namespace import binds nothing observable). Subsumes the old
    // fs/path/crypto-only `is_require_builtin_module` path. Non-literal
    // / unresolvable specifiers fall through to the legacy compile-time
    // refusal in `expr_call::intrinsics::try_require_literal`.
    if let Some(init_expr) = &decl.init {
        if let Some(module_name) = require_resolvable_native_specifier(init_expr) {
            register_require_namespace_binding(ctx, name, &module_name);
            return true;
        }
    }

    // Check if this is calling toString() on URLSearchParams - returns String
    if matches!(ty, Type::Any) {
        if let Some(init_expr) = &decl.init {
            if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
                if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                    if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                        if let ast::MemberProp::Ident(method_ident) = &member_expr.prop {
                            let method_name = method_ident.sym.as_ref();
                            if method_name == "toString" || method_name == "get" {
                                // Check if object is a URLSearchParams
                                if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                                    let obj_name = obj_ident.sym.as_ref();
                                    if let Some(obj_ty) = ctx.lookup_local_type(obj_name) {
                                        if matches!(obj_ty, Type::Named(name) if name == "URLSearchParams")
                                        {
                                            *ty = Type::String;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if this is assigning the result of a native method call that returns the same type
    // e.g., const sum = d1.plus(d2) where d1 is a Decimal -> sum should also be tracked as Decimal
    // Also handles: const r1 = new Big(...).div(...) patterns
    if let Some(init_expr) = &decl.init {
        if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
            if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                    let mut handled = false;
                    // First try: object is an ident that's a known native instance
                    if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                        let obj_name = obj_ident.sym.as_ref();
                        // Check if object is a native instance
                        if let Some((module, class)) = ctx.lookup_native_instance(obj_name) {
                            // Check if this method returns the same type (builder pattern)
                            if let ast::MemberProp::Ident(method_ident) = &member_expr.prop {
                                let method_name = method_ident.sym.as_ref();
                                // Methods that return the same type (Decimal, etc.)
                                let returns_same_type = match class {
                                    "Decimal" | "Big" | "BigNumber" => matches!(
                                        method_name,
                                        "plus"
                                            | "minus"
                                            | "times"
                                            | "div"
                                            | "mod"
                                            | "pow"
                                            | "sqrt"
                                            | "abs"
                                            | "neg"
                                            | "round"
                                            | "floor"
                                            | "ceil"
                                    ),
                                    _ => false,
                                };
                                if returns_same_type {
                                    ctx.register_native_instance(
                                        name.to_string(),
                                        module.to_string(),
                                        class.to_string(),
                                    );
                                    handled = true;
                                }
                            }
                        }
                    }
                    // Second try: object is new Big(...) or a chained call like new Big(...).div(...)
                    if !handled {
                        if let Some(module_name) =
                            detect_native_instance_expr(ctx, &member_expr.obj)
                        {
                            let class_name = match module_name {
                                "big.js" => "Big",
                                "decimal.js" => "Decimal",
                                "bignumber.js" => "BigNumber",
                                "lru-cache" => "LRUCache",
                                "commander" => "Command",
                                _ => "",
                            };
                            if !class_name.is_empty() {
                                ctx.register_native_instance(
                                    name.to_string(),
                                    module_name.to_string(),
                                    class_name.to_string(),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if this is assigning from fetch() or await fetch() - register as fetch Response
    if let Some(init_expr) = &decl.init {
        if crate::lower_types::is_node_readable_static_factory_call(ctx, init_expr) {
            let readable = "Readable".to_string();
            *ty = Type::Named(readable.clone());
            ctx.register_native_instance(name.to_string(), "stream".to_string(), readable);
        }

        // Check for: const response = fetch(url) / fetchWithAuth(url, auth) / fetchPostWithAuth(url, auth, body)
        if let Some(module) = get_fetch_module(init_expr) {
            ctx.register_native_instance(
                name.to_string(),
                module.to_string(),
                "Response".to_string(),
            );
        }
        // Check for: const response = await fetch(url) / await fetchWithAuth(...) / await fetchPostWithAuth(...)
        else if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
            if let Some(module) = get_fetch_module(&await_expr.arg) {
                ctx.register_native_instance(
                    name.to_string(),
                    module.to_string(),
                    "Response".to_string(),
                );
            }
        }

        // #5432: `const res = app.fetch(req)` / `await app.fetch(req)` —
        // a member-call `.fetch(...)` is the Fetch-API server-handler
        // convention (Hono `app.fetch`, itty-router, Cloudflare
        // Workers) and yields a native fetch Response. Record it in a
        // narrow set (NOT `register_native_instance`, which would hijack
        // every method on `res`) so only `res.headers.<m>()` bails the
        // array-method fold. See `fetch_call_response_locals`.
        if is_member_fetch_call(init_expr) {
            ctx.fetch_call_response_locals.insert(name.to_string());
        }

        // Web Fetch API: new Response(...) / new Headers(...) /
        // new Request(...) / new FormData(...)
        // Also handle Response.json(...) and Response.redirect(...) static factories.
        if let ast::Expr::New(new_expr) = init_expr.as_ref() {
            if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                // #6003: a user class/function/binding that happens to share a
                // Web-API constructor name (`class Headers { ... }`) lexically
                // shadows the global — `new Headers()` constructs the USER
                // class, so tagging the binding as a native instance here
                // would reroute every method call (`h.set(...)`) through the
                // native FFI and silently skip the user's methods. Same
                // shadowing rule as #5912's `new URL()` fix. The alias arm
                // below checks the RESOLVED name instead: the alias local
                // (`const B = Blob`) is itself a binding, but the name that
                // must be unshadowed is the underlying constructor.
                let ctor_shadowed = ctx.shadows_unqualified_global(class_ident.sym.as_ref());
                match class_ident.sym.as_ref() {
                    "Response" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "fetch".to_string(),
                            "Response".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    "Headers" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "Headers".to_string(),
                            "Headers".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    "Request" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "Request".to_string(),
                            "Request".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    "FormData" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "FormData".to_string(),
                            "FormData".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    // Issue #1211: `new Blob([...])` / `new File([...], name)`.
                    // File shares the Blob runtime registry — the codegen
                    // `module == "blob"` arm dispatches `.name` /
                    // `.lastModified` regardless of class tag, so File
                    // tracks as a Blob instance with the class tag
                    // available for future File-only property checks.
                    "Blob" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "blob".to_string(),
                            "Blob".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    "File" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "blob".to_string(),
                            "File".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    other
                        if ctx
                            .resolve_class_alias(other)
                            .as_deref()
                            .is_some_and(|resolved| {
                                matches!(resolved, "Blob" | "File")
                                    && !ctx.shadows_unqualified_global(resolved)
                            }) =>
                    {
                        let resolved = ctx.resolve_class_alias(other).unwrap();
                        ctx.register_native_instance(
                            name.to_string(),
                            "blob".to_string(),
                            resolved,
                        );
                        ctx.uses_fetch = true;
                    }
                    // Issue #237: Web Streams API constructors.
                    "ReadableStream" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "readable_stream".to_string(),
                            "ReadableStream".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    // #4915: `new ReadableStreamBYOBReader(stream)` —
                    // the handle is a reader, same module tag as
                    // `stream.getReader({ mode: "byob" })`.
                    "ReadableStreamBYOBReader" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "readable_stream_reader".to_string(),
                            "ReadableStreamBYOBReader".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    "WritableStream" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "writable_stream".to_string(),
                            "WritableStream".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    "TransformStream" if !ctor_shadowed => {
                        ctx.register_native_instance(
                            name.to_string(),
                            "transform_stream".to_string(),
                            "TransformStream".to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                    other => {
                        // Issue #562: `let x = new SubclassOfStream()`
                        // — walk the user class's `native_extends` to
                        // see if it points at a stream module. If so,
                        // register `x` under the same module/class
                        // tag the bare-stream constructor would. The
                        // codegen FFI sites unwrap the
                        // `__perry_stream_handle__` field at dispatch
                        // time, so a subclass instance and a bare
                        // numeric handle are interchangeable.
                        if let Some((module, class)) = ctx.lookup_class_native_extends(other) {
                            if matches!(
                                module,
                                "readable_stream" | "writable_stream" | "transform_stream"
                            ) {
                                ctx.register_native_instance(
                                    name.to_string(),
                                    module.to_string(),
                                    class.to_string(),
                                );
                                ctx.uses_fetch = true;
                            }
                        }
                    }
                }
            } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                let class_name = match &member.prop {
                    ast::MemberProp::Ident(prop_ident) => Some(prop_ident.sym.as_ref()),
                    ast::MemberProp::Computed(prop) => match prop.expr.as_ref() {
                        ast::Expr::Lit(ast::Lit::Str(s)) => s.value.as_str(),
                        _ => None,
                    },
                    _ => None,
                };
                let is_blob_file_ctor = match member.obj.as_ref() {
                    ast::Expr::Ident(obj_ident) if obj_ident.sym.as_ref() == "globalThis" => true,
                    ast::Expr::Ident(obj_ident) => ctx
                        .lookup_native_module(obj_ident.sym.as_ref())
                        .is_some_and(|(module, _)| module == "buffer" || module == "node:buffer"),
                    _ => false,
                };
                if is_blob_file_ctor {
                    if let Some(class_name @ ("Blob" | "File")) = class_name {
                        ctx.register_native_instance(
                            name.to_string(),
                            "blob".to_string(),
                            class_name.to_string(),
                        );
                        ctx.uses_fetch = true;
                    }
                }
            }
        }
        // Response.json(...) / Response.redirect(...) static factories
        if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
            if let ast::Callee::Expr(callee) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if obj_ident.sym.as_ref() == "Response" {
                            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                match prop_ident.sym.as_ref() {
                                    "json" | "redirect" | "error" => {
                                        ctx.register_native_instance(
                                            name.to_string(),
                                            "fetch".to_string(),
                                            "Response".to_string(),
                                        );
                                        ctx.uses_fetch = true;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
        // Response.clone() — for: const r5clone = r5.clone();
        // The result is a new Response. Detect by checking if the receiver is already
        // a fetch::Response native instance.
        if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
            if let ast::Callee::Expr(callee) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            if prop_ident.sym.as_ref() == "clone" {
                                if let Some((m, c)) =
                                    ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                {
                                    if c == "Response" {
                                        let m = m.to_string();
                                        ctx.register_native_instance(
                                            name.to_string(),
                                            m,
                                            "Response".to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Issue #234 / fetch body helpers: const blob = await <res|req>.blob()
        // registers Blob results; const form = await <res|req>.formData()
        // registers FormData results so follow-up calls dispatch through
        // the typed fetch lowering instead of the generic handle path.
        if let ast::Expr::Await(await_expr) = init_expr.as_ref() {
            if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                if let ast::Callee::Expr(callee) = &call_expr.callee {
                    if let ast::Expr::Member(member) = callee.as_ref() {
                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                            if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                                match prop_ident.sym.as_ref() {
                                    "blob" => {
                                        if let Some((_, c)) =
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                        {
                                            if c == "Response" || c == "Request" {
                                                ctx.register_native_instance(
                                                    name.to_string(),
                                                    "blob".to_string(),
                                                    "Blob".to_string(),
                                                );
                                            }
                                        }
                                    }
                                    "formData" => {
                                        if let Some((_, c)) =
                                            ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                        {
                                            if c == "Response" || c == "Request" {
                                                ctx.register_native_instance(
                                                    name.to_string(),
                                                    "FormData".to_string(),
                                                    "FormData".to_string(),
                                                );
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
        // Issue #234: const b2 = blob.slice(...) — chained slicing
        // returns a new Blob. Detect when the receiver is already a
        // blob::Blob native instance.
        if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
            if let ast::Callee::Expr(callee) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            if prop_ident.sym.as_ref() == "slice" {
                                if let Some((_, c)) =
                                    ctx.lookup_native_instance(obj_ident.sym.as_ref())
                                {
                                    if c == "Blob" {
                                        ctx.register_native_instance(
                                            name.to_string(),
                                            "blob".to_string(),
                                            "Blob".to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Issue #237: Web Streams chained-typed-method bindings.
        // Recognize chained method/property forms that return a new
        // streams native instance so subsequent dispatch routes to
        // the right `module == "..."` arm in lower_call.rs.
        if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
            if let ast::Callee::Expr(callee) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            let m = prop_ident.sym.as_ref().to_string();
                            let class_owned = ctx
                                .lookup_native_instance(obj_ident.sym.as_ref())
                                .map(|(_, c)| c.to_string());
                            if let Some(c) = class_owned {
                                if m == "stream" && c == "Blob" {
                                    ctx.register_native_instance(
                                        name.to_string(),
                                        "readable_stream".to_string(),
                                        "ReadableStream".to_string(),
                                    );
                                }
                                if m == "getReader" && c == "ReadableStream" {
                                    ctx.register_native_instance(
                                        name.to_string(),
                                        "readable_stream_reader".to_string(),
                                        "ReadableStreamDefaultReader".to_string(),
                                    );
                                }
                                if m == "getWriter" && c == "WritableStream" {
                                    ctx.register_native_instance(
                                        name.to_string(),
                                        "writable_stream_writer".to_string(),
                                        "WritableStreamDefaultWriter".to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Issue #237: const stream = response.body / const r = ts.readable / .writable
        // Property reads on a native instance — destructured as Member
        // expressions (no Call wrapper).
        if let ast::Expr::Member(member) = init_expr.as_ref() {
            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                    let p = prop_ident.sym.as_ref().to_string();
                    let class_owned = ctx
                        .lookup_native_instance(obj_ident.sym.as_ref())
                        .map(|(_, c)| c.to_string());
                    if let Some(c) = class_owned {
                        if p == "body" && c == "Response" {
                            ctx.register_native_instance(
                                name.to_string(),
                                "readable_stream".to_string(),
                                "ReadableStream".to_string(),
                            );
                        }
                        if p == "readable" && c == "TransformStream" {
                            ctx.register_native_instance(
                                name.to_string(),
                                "readable_stream".to_string(),
                                "ReadableStream".to_string(),
                            );
                        }
                        if p == "writable" && c == "TransformStream" {
                            ctx.register_native_instance(
                                name.to_string(),
                                "writable_stream".to_string(),
                                "WritableStream".to_string(),
                            );
                        }
                    }
                }
            }
        }

        // Issue #237: const stream = upstream.pipeThrough(transform)
        // returns a ReadableStream (the transform's readable side).
        if let ast::Expr::Call(call_expr) = init_expr.as_ref() {
            if let ast::Callee::Expr(callee) = &call_expr.callee {
                if let ast::Expr::Member(member) = callee.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                        if let ast::MemberProp::Ident(prop_ident) = &member.prop {
                            if prop_ident.sym.as_ref() == "pipeThrough" {
                                let class_owned = ctx
                                    .lookup_native_instance(obj_ident.sym.as_ref())
                                    .map(|(_, c)| c.to_string());
                                if class_owned.as_deref() == Some("ReadableStream") {
                                    ctx.register_native_instance(
                                        name.to_string(),
                                        "readable_stream".to_string(),
                                        "ReadableStream".to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if calling a function whose return type is a native module type
    // e.g., const dbPool = initializePool() where initializePool(): mysql.Pool
    // Also handles: const dbPool = await initializePool()
    if let Some(init_expr) = &decl.init {
        let call_expr = match init_expr.as_ref() {
            ast::Expr::Call(c) => Some(c),
            ast::Expr::Await(await_expr) => {
                if let ast::Expr::Call(c) = await_expr.arg.as_ref() {
                    Some(c)
                } else {
                    None
                }
            }
            _ => None,
        };
        // Variable-to-variable propagation for native instances
        // (`let sock: Socket = plainSock`) is handled by the
        // post-lowering cross-module pass; see
        // `js_transform::scan_for_ident_init_propagation`.
        if let Some(call_expr) = call_expr {
            if let ast::Callee::Expr(callee_expr) = &call_expr.callee {
                // Check direct function calls: const x = someFunc()
                if let ast::Expr::Ident(func_ident) = callee_expr.as_ref() {
                    let func_name = func_ident.sym.as_ref();
                    if let Some((module, class)) = ctx.lookup_func_return_native_instance(func_name)
                    {
                        ctx.register_native_instance(
                            name.to_string(),
                            module.to_string(),
                            class.to_string(),
                        );
                    }
                }
                // Check method calls on native instances: const conn = pool.getConnection()
                if let ast::Expr::Member(member_expr) = callee_expr.as_ref() {
                    if let ast::Expr::Ident(obj_ident) = member_expr.obj.as_ref() {
                        let obj_name = obj_ident.sym.as_ref();
                        if let Some((module, class)) = ctx.lookup_native_instance(obj_name) {
                            if let ast::MemberProp::Ident(method_ident) = &member_expr.prop {
                                let method_name = method_ident.sym.as_ref();
                                // Map method calls to their return types
                                let return_class = match (module, class, method_name) {
                                    ("mysql2" | "mysql2/promise", "Pool", "getConnection") => {
                                        Some("PoolConnection")
                                    }
                                    ("pg", "Pool", "connect") => Some("Client"),
                                    _ => None,
                                };
                                if let Some(ret_class) = return_class {
                                    ctx.register_native_instance(
                                        name.to_string(),
                                        module.to_string(),
                                        ret_class.to_string(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}
