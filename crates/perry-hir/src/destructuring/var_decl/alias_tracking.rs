//! Alias / prototype / static-method tracking for a simple `let/const/var`
//! identifier binding (extracted from `var_decl.rs`'s `Pat::Ident` arm).

use super::*;

use crate::types::LocalId;
use swc_ecma_ast as ast;

use crate::lower::LoweringContext;

/// Records the various alias/prototype/static-method facts a freshly-bound
/// simple identifier (`id`, lowered `init`) carries. Pure side effects on
/// `ctx`; mirrors the original inline block verbatim.
pub(crate) fn track_decl_aliases(
    ctx: &mut LoweringContext,
    decl: &ast::VarDeclarator,
    name: &str,
    id: LocalId,
    init: &Option<Expr>,
) {
    // Issue #886: detect `let/const/var <name> = Object.<staticMethod>`
    // from the raw AST so a subsequent indirect call `<name>(args)`
    // can route to the dedicated HIR variant the literal
    // `Object.<staticMethod>(args)` already uses. The detection runs
    // from the AST (rather than the lowered `init`) because the init
    // lowering erases the `Object` qualifier into a generic
    // PropertyGet that resolves to undefined at codegen. esbuild's
    // CJS-bundle prelude emits this pattern verbatim for every
    // bundled package:
    //   var __defProp = Object.defineProperty;
    //   var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
    //   var __getOwnPropNames = Object.getOwnPropertyNames;
    //   var __getProtoOf = Object.getPrototypeOf;
    //   var __defProps = Object.defineProperties;
    // — so anything that imports an esbuild-bundled package threw
    // `TypeError: value is not a function` at module init pre-fix.
    let object_method_alias: Option<String> =
        decl.init.as_deref().and_then(|init_ast| match init_ast {
            ast::Expr::Member(member) => match (member.obj.as_ref(), &member.prop) {
                (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                    if obj_ident.sym.as_ref() == "Object" =>
                {
                    let method_name = method_ident.sym.as_ref();
                    // Whitelist of static methods that already have
                    // a dedicated HIR variant in `lower/expr_call.rs`.
                    // Methods not on this list intentionally fall
                    // through to the generic PropertyGet path so we
                    // don't change behaviour for unsupported ones.
                    let is_supported = matches!(
                        method_name,
                        "defineProperty"
                            | "defineProperties"
                            | "setPrototypeOf"
                            | "getPrototypeOf"
                            | "getOwnPropertyDescriptor"
                            | "getOwnPropertyDescriptors"
                            | "getOwnPropertyNames"
                            | "getOwnPropertySymbols"
                            | "keys"
                            | "values"
                            | "entries"
                            | "assign"
                            | "fromEntries"
                            | "create"
                            | "freeze"
                            | "seal"
                            | "preventExtensions"
                            | "isFrozen"
                            | "isSealed"
                            | "isExtensible"
                            | "hasOwn"
                            | "is"
                    );
                    if is_supported {
                        Some(method_name.to_string())
                    } else {
                        None
                    }
                }
                (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                    if obj_ident.sym.as_ref() == "Array"
                        && method_ident.sym.as_ref() == "isArray" =>
                {
                    Some("Array.isArray".to_string())
                }
                (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                    if matches!(method_ident.sym.as_ref(), "json" | "redirect" | "error") && {
                        let obj_name = obj_ident.sym.as_ref();
                        (obj_name == "Response" && ctx.lookup_local("Response").is_none())
                            || ctx
                                .resolve_class_alias(obj_name)
                                .as_deref()
                                .is_some_and(|resolved| resolved == "Response")
                    } =>
                {
                    let method = match method_ident.sym.as_ref() {
                        "json" => "Response.static_json",
                        "redirect" => "Response.static_redirect",
                        "error" => "Response.static_error",
                        _ => unreachable!(),
                    };
                    Some(method.to_string())
                }
                _ => None,
            },
            _ => None,
        });
    let array_method_alias: Option<String> =
        decl.init.as_deref().and_then(|init_ast| match init_ast {
            ast::Expr::Member(member) => match (member.obj.as_ref(), &member.prop) {
                (ast::Expr::Ident(obj_ident), ast::MemberProp::Ident(method_ident))
                    if obj_ident.sym.as_ref() == "Array" =>
                {
                    let method_name = method_ident.sym.as_ref();
                    if method_name == "isArray" {
                        Some(method_name.to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        });

    // Issue #886: register the alias once `id` is bound, so the
    // call-side recogniser in `lower/expr_call.rs` can route
    // `LocalGet(id)(args)` to the dedicated HIR variant the literal
    // `Object.<method>(args)` shape already uses.
    if let Some(method_name) = object_method_alias {
        ctx.object_static_method_aliases.insert(id, method_name);
    }
    if let Some(method_name) = array_method_alias {
        ctx.array_static_method_aliases.insert(id, method_name);
    }
    if let Some(Expr::NativeMethodCall { module, method, .. }) = &init {
        if module == "fetch"
            && matches!(
                method.as_str(),
                "static_json" | "static_redirect" | "static_error"
            )
        {
            ctx.register_native_instance(
                name.to_string(),
                "fetch".to_string(),
                "Response".to_string(),
            );
            ctx.uses_fetch = true;
        }
    }

    // Issue #740: track `let/const/var <name> = ClassRef(...)` so
    // `new <name>(...)` can resolve captures via the alias chain.
    // Also follow LocalGet aliases for `const B = A` style chains.
    if let Some(init_expr) = &init {
        // Issue #838 followup (b): tag locals that hold a
        // callable value at runtime. Inside an IIFE the AST
        // pattern `function M(t){…}` hoists to a `Let { name:
        // "M", init: Some(Closure{…}) }`; the matching
        // `M.prototype.x = fn` site needs to resolve `M`'s
        // local id through this set so the
        // prototype-method recogniser routes through the
        // function-classic path. Also covers
        // `var Klass = function(){…}` (anonymous function
        // expression assigned to a local).
        if matches!(init_expr, Expr::Closure { .. } | Expr::FuncRef(_)) {
            ctx.function_valued_locals.insert(id);
        }
        if is_global_this_value(ctx, init_expr) {
            ctx.global_this_aliases.insert(id);
        }
        match init_expr {
            Expr::ClassRef(class_name) => {
                ctx.register_let_class_alias(name.to_string(), class_name.clone());
            }
            Expr::LocalGet(src_id) => {
                if let Some((src_name, _, _)) =
                    ctx.locals.iter().rev().find(|(_, lid, _)| lid == src_id)
                {
                    let src_name = src_name.clone();
                    if let Some(resolved) = ctx.resolve_class_alias(&src_name) {
                        ctx.register_let_class_alias(name.to_string(), resolved);
                    } else if ctx.classes_index.contains_key(&src_name) {
                        ctx.register_let_class_alias(name.to_string(), src_name);
                    }
                }
                // Issue #838: follow prototype-alias chains too,
                // so `var m = M.prototype; var n = m; n.foo = …`
                // still recognises the underlying class.
                if let Some(class_name) = ctx.prototype_aliases.get(src_id).cloned() {
                    ctx.prototype_aliases.insert(id, class_name);
                }
                // Issue #838 followup (b): same chain follow for
                // function-decl prototype aliases.
                if let Some(func_id) = ctx.prototype_function_aliases.get(src_id).copied() {
                    ctx.prototype_function_aliases.insert(id, func_id);
                }
                if let Some(src_local) = ctx.prototype_function_locals.get(src_id).copied() {
                    ctx.prototype_function_locals.insert(id, src_local);
                }
                // Propagate function-valued tag through aliases.
                if ctx.function_valued_locals.contains(src_id) {
                    ctx.function_valued_locals.insert(id);
                }
                // Issue #886: propagate the Object-static-method alias
                // through `const B = A` chains so re-aliased copies
                // (`const __defProp2 = __defProp;`) still route to the
                // dedicated HIR variant at the indirect call site.
                if let Some(method_name) = ctx.object_static_method_aliases.get(src_id).cloned() {
                    ctx.object_static_method_aliases.insert(id, method_name);
                }
                if let Some(method_name) = ctx.array_static_method_aliases.get(src_id).cloned() {
                    ctx.array_static_method_aliases.insert(id, method_name);
                }
            }
            Expr::PropertyGet {
                object, property, ..
            } if is_global_this_value(ctx, object.as_ref())
                && matches!(
                    property.as_str(),
                    "URL"
                        | "URLSearchParams"
                        | "TextEncoder"
                        | "TextDecoder"
                        | "Blob"
                        | "File"
                        | "FormData"
                        | "Headers"
                        | "Request"
                        | "Response"
                        | "WebSocket"
                ) =>
            {
                ctx.register_let_class_alias(name.to_string(), property.clone());
                if matches!(
                    property.as_str(),
                    "Blob" | "File" | "FormData" | "Headers" | "Request" | "Response"
                ) {
                    ctx.uses_fetch = true;
                }
            }
            Expr::PropertyGet {
                object, property, ..
            } if matches!(object.as_ref(), Expr::NativeModuleRef(module)
                    if module == "buffer" || module == "node:buffer")
                && matches!(property.as_str(), "Blob" | "File") =>
            {
                ctx.register_let_class_alias(name.to_string(), property.clone());
                ctx.uses_fetch = true;
            }
            // Issue #838: `var p = <ClassName>.prototype` records
            // the alias so a later `p.<method> = <fn>` lowers to
            // RegisterPrototypeMethod. dayjs's minified shape
            // (`var m = M.prototype; m.parse = function(){…};
            //  m.init = function(){…};`) hits this — without
            // alias-tracking the assignments fell through to a
            // generic PropertySet on the prototype proxy that
            // nothing downstream observed.
            //
            // Issue #838 followup (b): same shape but the base is
            // a function declaration (Babel's class-from-function
            // emit pattern, also what dayjs's minified `function
            // M(){}; var m = M.prototype` lowers to). Tracked
            // separately in `prototype_function_aliases` so the
            // assignment recogniser can route to the
            // function-flavoured prototype-method registration
            // path (synthetic class id allocated at runtime).
            Expr::PropertyGet {
                object, property, ..
            } if property == "prototype" => {
                match object.as_ref() {
                    Expr::ClassRef(class_name) => {
                        ctx.prototype_aliases.insert(id, class_name.clone());
                    }
                    Expr::FuncRef(func_id) => {
                        ctx.prototype_function_aliases.insert(id, *func_id);
                    }
                    // dayjs's minified IIFE shape lowers the inner
                    // `function M(t){…}` to a `Let { name: "M", init:
                    // Some(Closure{…}) }` (function decls inside a
                    // function expression body become hoisted lets in
                    // HIR). The subsequent `var m = M.prototype` then
                    // reads `M` as `LocalGet(M_id)` — match that and
                    // route the alias through the same function-class
                    // bucket, storing the receiver local id so the
                    // recogniser later emits
                    // `RegisterFunctionPrototypeMethod { func:
                    // LocalGet(M_id), … }`.
                    Expr::LocalGet(src_local) => {
                        if ctx.function_valued_locals.contains(src_local) {
                            ctx.prototype_function_locals.insert(id, *src_local);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
