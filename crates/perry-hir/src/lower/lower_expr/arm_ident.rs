//! The `ast::Expr::Ident` arm of `lower_expr_impl`, extracted to a helper.
//! Pure code move — no behavior change.

use super::*;
use crate::types::Type;
use anyhow::Result;
use swc_ecma_ast as ast;

fn class_binding_read(ctx: &LoweringContext, source_name: &str) -> Expr {
    let class_name = ctx.resolve_class_name(source_name);
    // Keep the normal, overwhelmingly-common class read as a ClassRef
    // immediate. Besides preserving static class recognition downstream, it
    // prevents the GC root pass from treating a runtime-call result containing
    // an INT32-tagged ClassRef as a heap pointer. Only a class declaration
    // actually reassigned from a closure needs the mutable side-table bridge.
    if !ctx.reassigned_top_level_identifiers.contains(source_name) {
        return Expr::ClassRef(class_name);
    }
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_class_lexical_binding_get".to_string(),
            param_types: vec![Type::Any],
            return_type: Type::Any,
        }),
        args: vec![Expr::ClassRef(class_name)],
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

pub(crate) fn lower_ident_expr(ctx: &mut LoweringContext, ident: &ast::Ident) -> Result<Expr> {
    let expr_ident = ast::Expr::Ident(ident.clone());
    let expr = &expr_ident;
    let name = ident.sym.to_string();
    // A named import from a node-core module that is not a value export of that
    // module was deferred (neither bound nor rejected) at import lowering — see
    // `LoweringContext::deferred_unknown_native_imports`. Type annotations are
    // erased before expression lowering, so reaching this arm means the local is
    // genuinely used as a VALUE (`import { nope } from "crypto"; nope()`), not a
    // TS type in a mixed import (`import { createCipheriv, BinaryLike }`). Raise
    // the original diagnostic now, at the real point of use. A same-named local
    // that shadows the specifier resolves normally and is untouched.
    if ctx.lookup_local(&name).is_none() {
        if let Some((imported, raw_source, span)) =
            ctx.deferred_unknown_native_imports.get(&name).cloned()
        {
            crate::lower_bail!(
                span,
                "The requested module '{}' does not provide an export named '{}'",
                raw_source,
                imported
            );
        }
    }
    let with_envs = ctx.active_with_envs_for_ident(&name);
    if !with_envs.is_empty() {
        let saved_with_envs = std::mem::take(&mut ctx.with_env_stack);
        let fallback = lower_expr(ctx, expr);
        ctx.with_env_stack = saved_with_envs;
        return Ok(wrap_with_gets(&name, fallback?, with_envs));
    }
    // A class body has its own immutable lexical binding for the class name.
    // It shadows an outer local of the same name; only a parameter/local
    // introduced inside the method body may shadow it in turn.
    let nearer_local = ctx
        .local_decl_scope_depth(&name)
        .zip(ctx.current_class_scope_depth)
        .is_some_and(|(local_depth, class_depth)| local_depth > class_depth);
    // A repeatedly evaluated named class expression has a real lexical value,
    // not merely its shared template ClassRef. Keep that value in a
    // compiler-private local so its own members use this evaluation and a
    // nested class can capture an outer class expression's binding. Search
    // innermost-first to preserve ordinary lexical shadowing.
    if let Some((_, binding_depth, binding_id)) = ctx
        .class_expr_self_bindings
        .iter()
        .rev()
        .find(|(binding_name, _, _)| binding_name == &name)
    {
        let shadowed_by_nearer_local = ctx
            .local_decl_scope_depth(&name)
            .is_some_and(|local_depth| local_depth > *binding_depth);
        if !shadowed_by_nearer_local {
            return Ok(Expr::LocalGet(*binding_id));
        }
    }
    if ctx.current_class_inner_name.as_deref() == Some(name.as_str()) && !nearer_local {
        if let Some(current) = ctx.current_class.clone() {
            return Ok(Expr::ClassRef(current));
        }
    }
    // A class declared in the current function body lexically shadows a
    // same-named binding from an OUTER scope. Resolution normally checks
    // `lookup_local` (which finds outer-scope locals) before the class,
    // so without this a nested `class a` whose name also exists as an
    // outer local resolved to that outer local. In the Next.js app-page
    // bundle a webpack chunk's `a` (`a=()=>{}`, undefined at module-init
    // time) is captured into a module factory that declares
    // `class a extends Error` (p-timeout's TimeoutError); the export
    // `e.exports.TimeoutError=a` then read the outer `undefined` instead
    // of the class, so `new r.TimeoutError` threw "undefined is not a
    // constructor". Gate on there being NO current-scope local of that
    // name (a sibling param/var/let still wins).
    // A `class <name>` in `forward_class_names` shadows a SAME-named local
    // only when JS lexical scoping says the class binding is the nearest: no
    // local of that name at all, or the class declared at a scope depth deeper
    // than (nearer the reference than) the nearest enclosing local, and no
    // sibling param/var of that name in the CURRENT scope. The class binding
    // lives in whatever function body declared it; a captured local declared in
    // a NEARER scope (a deeper or sibling function whose local lingered in the
    // inherited set) must win. Without the depth check, a `class <name>` in a
    // SIBLING factory (its name still present in the inherited
    // `forward_class_names`) wrongly shadowed a legitimate captured local —
    // Next.js app-page-turbo's route-render closure read its captured params
    // local `ej` as the `class ej` (= NextURL) reference, which then flowed
    // into a WeakMap key and threw "Invalid value used as weak map key".
    // (`forward_class_shadows_local` is shared with the `new <Ident>` arm; see
    // #8040 for what happened while the two disagreed.)
    if ctx.forward_class_shadows_local(&name) {
        return Ok(class_binding_read(ctx, &name));
    }
    // Chained-assignment class self-alias referenced from inside one of the
    // class's own method/getter/setter bodies. tsc's decorator-capture form
    // `let Logger = Logger_1 = class Logger { get x(){ …Logger_1… } static
    // error(){ …Logger_1… } }` declares `Logger_1` as a real outer-scope local
    // and binds the class to it. Inside a STATIC method the outer local is not
    // in scope (the method compiles to a standalone function), and inside an
    // instance method resolving to a `LocalGet` capture forces the whole class
    // onto the per-evaluation `ClassExprFresh` path (which drops static methods
    // and SIGSEGVs `new`). The alias value IS the class everywhere after
    // evaluation, so resolve it to the constructor `ClassRef` directly — exactly
    // as JS spec resolves the inner class binding. `record_chained_class_self_
    // aliases` populates `class_expr_aliases[alias] = <class lowering name>`
    // before the body is lowered; gate on currently lowering that same class so
    // an unrelated outer reference to the name is untouched. (`class_expr_
    // aliases` is NOT a `register_class`, so this does not trip
    // `lower_class_expr`'s collision-rename — `current_class` stays the real
    // class name.)
    if let Some(cur) = ctx.current_class.clone() {
        if let Some(target) = ctx.class_expr_aliases.get(&name) {
            // Compare the RESOLVED target name: a collision-renamed class
            // expression makes `current_class` the resolved name while the
            // recorded `target` is still the source name, so a raw `*target ==
            // cur` would miss. Resolving is identity when no rename happened.
            let resolved = ctx.resolve_class_name(target);
            if resolved == cur {
                return Ok(Expr::ClassRef(resolved));
            }
        }
    }
    if let Some(id) = ctx.lookup_local(&name) {
        // A with-fallback implicit global may still be the HOLE
        // sentinel (the with-env took the write) — reading it then
        // is a ReferenceError, not undefined.
        if let Some(n) = ctx.with_sloppy_implicit_ids.get(&id) {
            return Ok(Expr::Call {
                callee: Box::new(Expr::ExternFuncRef {
                    name: "js_with_implicit_read".to_string(),
                    param_types: vec![Type::Any, Type::String],
                    return_type: Type::Any,
                }),
                args: vec![Expr::LocalGet(id), Expr::String(n.clone())],
                type_args: vec![],
                byte_offset: 0,
            });
        }
        Ok(Expr::LocalGet(id))
    } else if let Some(id) = ctx.lookup_func(&name) {
        Ok(Expr::FuncRef(id))
    } else if ctx.lookup_native_module(&name).is_some() {
        Ok(native_module_binding_value(ctx, &name))
    } else if let Some(orig_name) = ctx.lookup_imported_func(&name) {
        // Imported function - reference by its original exported name
        // Look up type information if available
        let (param_types, return_type) = ctx
            .lookup_extern_func_types(orig_name)
            .map(|(p, r)| (p.clone(), r.clone()))
            .unwrap_or_else(|| (Vec::new(), Type::Any));
        Ok(Expr::ExternFuncRef {
            name: orig_name.to_string(),
            param_types,
            return_type,
        })
    } else if is_builtin_function(&name) {
        // Built-in global function (setTimeout, etc.)
        Ok(Expr::ExternFuncRef {
            name,
            param_types: Vec::new(),
            return_type: Type::Any,
        })
    } else if ctx.lookup_class(&name).is_some() {
        // Class used as a first-class value (e.g., { Point: Point })
        Ok(class_binding_read(ctx, &name))
    } else if ctx.forward_class_names.contains(&name) {
        // Forward reference to a sibling class declared LATER in the
        // same function body (vendored zod: ZodType.optional() →
        // ZodOptional.create(...)). JS resolves this at call time;
        // emit a ClassRef by name — codegen resolves it from the
        // class registry, which has every pending class by then.
        Ok(class_binding_read(ctx, &name))
    } else if name == "undefined" {
        // Global undefined identifier
        Ok(Expr::Undefined)
    } else if name == "null" {
        // Global null identifier (though typically written as literal)
        Ok(Expr::Null)
    } else if name == "NaN" {
        // Global NaN identifier
        Ok(Expr::Number(f64::NAN))
    } else if name == "Infinity" {
        // Global Infinity identifier
        Ok(Expr::Number(f64::INFINITY))
    } else if name == "__dirname" || name == "__filename" {
        // Issue #667: CJS-style module locals. Without this fold,
        // the bare reference falls through to GlobalGet(0) -> 0,
        // which silently corrupts any path computation built on
        // path.join(__dirname, ...). Mirrors the import.meta arm
        // (expr_misc::import_meta_paths) so both surfaces agree.
        let path = ctx.source_file_path.replace('\\', "/");
        let value = if name == "__filename" {
            path.clone()
        } else {
            match path.rfind('/') {
                Some(i) if i > 0 => path[..i].to_string(),
                Some(_) => "/".to_string(),
                None => String::new(),
            }
        };
        Ok(Expr::String(value))
    } else if matches!(name.as_str(), "Math" | "JSON" | "Reflect" | "Intl") {
        // #4139: the built-in namespace objects used as VALUES (passed
        // to `Object.getOwnPropertyDescriptor(Math, …)`, stored in a
        // local, etc.) must resolve to the real
        // `populate_global_this_builtins`-installed namespace object —
        // not the bare `GlobalGet(0)` sentinel (which IS `globalThis`,
        // so `Math === globalThis` and reflection reads the wrong
        // object). Reuse the `PropertyGet { GlobalGet(0), <name> }`
        // value-form (same as the built-in constructors above). When
        // these names appear in member-OBJECT position (`Math.max(…)`,
        // `Math.PI`), expr_member.rs's #973 reroute-undo resets the
        // receiver back to `GlobalGet(0)`, so the intrinsic call /
        // constant-fold paths are unchanged. A shadowing local would
        // have matched `ctx.lookup_local` earlier and never reached
        // here.
        Ok(Expr::PropertyGet {
            byte_offset: 0,
            object: Box::new(Expr::GlobalGet(0)),
            property: name,
        })
    } else if name == "require" && ctx.is_external_module {
        // Tier 1 of #5389 (fixes #5373): compiled external /
        // compilePackages modules carry no ambient CJS `require`
        // binding, so a bare or computed `require(expr)` would fall
        // through to the `js_global_get_or_throw_unresolved` arm below
        // and throw `ReferenceError: require is not defined`. Bind a
        // bare unshadowed `require` to a real createRequire-backed
        // closure instead — builtins (`node:os`, …) resolve by string;
        // unresolved package/file specifiers throw Node-compatible
        // MODULE_NOT_FOUND. Reaching this arm means
        // `require` is unshadowed (a local/func/imported/native binding
        // would have matched an earlier arm). Gated to external modules:
        // in first-party source the bare-require compile error (#668)
        // is deliberate and must not regress into a runtime path.
        Ok(Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_module_ambient_require".to_string(),
                param_types: Vec::new(),
                return_type: Type::Any,
            }),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        })
    } else {
        // GlobalGet(0) is a sentinel: codegen routes by name from the
        // parent PropertyGet/Call/Member context. Bare uses lower to
        // 0.0 (perry-codegen/src/expr.rs Expr::GlobalGet arm).
        let known_global = is_known_global_identifier_name(&name);
        if !known_global {
            // A global created at RUNTIME (sloppy `this.y = 2` with
            // `this` = globalThis inside a dynamic function) is
            // invisible to compile-time resolution — look it up on
            // globalThis first; only a true miss throws the spec
            // ReferenceError, with the identifier in the message.
            //
            // #6652: this by-name runtime lookup applies in member-OBJECT
            // position too (`ctx.unresolved_ident_as_global`). Pre-fix,
            // member-object lowering collapsed the unknown ident to the bare
            // `GlobalGet(0)` sentinel — which IS globalThis — so the ident
            // name was discarded and the MEMBER dispatched against
            // globalThis: `hasOwnProperty.call(o, k)` read `globalThis.call`
            // (undefined → "TypeError: value is not a function", @babel/types
            // placeholders.js in the pi bundle) and a runtime-created
            // `myGlobal.prop` read `globalThis.prop`. The lookup resolves
            // through `js_object_get_field_by_name` on globalThis, which
            // serves Object.prototype-INHERITED members (`hasOwnProperty`,
            // `toString`, `valueOf`, …) with identity preserved — exactly
            // Node's global-scope resolution. Bare CALLS of such idents get
            // `this = undefined` (spec: the global environment record's
            // WithBaseObject is undefined; Node: `toString()` →
            // "[object Undefined]" even in sloppy CJS), which the generic
            // call path already provides.
            if ctx.unresolved_ident_as_global {
                eprintln!(
                    "  Warning: unknown identifier '{}' in {} — assuming global; resolved by name on globalThis (incl. Object.prototype-inherited members) at runtime",
                    name,
                    ctx.source_file_path
                );
            }
            // #5253: localize the `X is not defined` ReferenceError to
            // this identifier's source position (winston `module`).
            return Ok(super::helpers::unresolved_global_get_expr(
                name.clone(),
                ident.span.lo.0,
            ));
        }
        // Bare built-in constructor identifiers (`Date`, `Array`,
        // `Object`, ...) used as VALUES (not method receivers /
        // `new` callees) need a real closure pointer so identity
        // comparisons like `inst.constructor === Date` hold —
        // both sides must resolve to the same `populate_global_this_builtins`-
        // installed closure. Reuse the existing
        // `PropertyGet { GlobalGet, <name> }` codegen path that
        // dispatches through `js_get_global_this` for builtin
        // names. Bare-callee shapes (e.g. `Date.now()`, `new
        // Date()`) are picked off earlier by their dedicated HIR
        // variants — `Expr::DateNow`, `Expr::DateNew(...)`,
        // `Expr::Date*Get(...)` — so they don't reach this arm.
        // date-fns / drizzle / lodash duck-typing path.
        if is_builtin_global_value_name(&name) {
            if is_fetch_global_value_name(&name) {
                ctx.uses_fetch = true;
            }
            return Ok(Expr::PropertyGet {
                byte_offset: 0,
                object: Box::new(Expr::GlobalGet(0)),
                property: name,
            });
        }
        Ok(Expr::GlobalGet(0))
    }
}
