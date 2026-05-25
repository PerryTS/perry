//! AST to HIR lowering — extracted from `lower/mod.rs` (issue #1101).
//!
//! Pure mechanical split: no logic changes. Helpers keep their original
//! visibility and are re-exported from `lower/mod.rs` so the existing
//! `expr_*` submodules and the rest of the crate keep compiling unchanged.

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

pub(crate) fn lower_module_decl(
    ctx: &mut LoweringContext,
    module: &mut Module,
    decl: &ast::ModuleDecl,
) -> Result<()> {
    match decl {
        ast::ModuleDecl::Import(import_decl) => {
            // Get the source module path
            let raw_source = import_decl.src.value.as_str().unwrap_or("").to_string();
            // Normalize "node:" prefix (e.g., "node:async_hooks" -> "async_hooks")
            let source = raw_source
                .strip_prefix("node:")
                .unwrap_or(&raw_source)
                .to_string();

            if source == "reflect-metadata" {
                emit_reflect_metadata_shim_note();
                return Ok(());
            }

            // Check if this is a native module import
            let is_native = is_native_module(&source);

            // Native modules have no class metadata to extract — `node:fs`,
            // `node:path`, etc. produce no `ImportedClass` entries and the
            // runtime can't resolve type-only names anyway. Skip the whole
            // declaration for native modules.
            //
            // For TypeScript modules, fall through and process the
            // declaration even when `type_only` is set: the named-specifier
            // loop below treats a whole-`import type { ... }` like a
            // per-specifier `import { type ... }` (which already flowed
            // class info into `imported_classes` since the v0.5.405 fix).
            // Without this, `import type { Foo }` dropped Foo's class
            // metadata before it reached compile.rs, so codegen lost the
            // method registry — `obj.method()` worked only via the
            // CLASS_VTABLE_REGISTRY runtime fallback (#392 followup) and
            // `typeof obj.method` returned `"undefined"`. Issue #446.
            if import_decl.type_only && is_native {
                return Ok(());
            }
            let whole_decl_type_only = import_decl.type_only;

            // Parse import specifiers
            let mut specifiers = Vec::new();
            for spec in &import_decl.specifiers {
                match spec {
                    ast::ImportSpecifier::Named(named) => {
                        // Skip individual type-only specifiers (`import { type Foo,
                        // Bar }`) only for native module imports — there's no
                        // class info to extract from `node:fs`, etc., and the
                        // runtime can't resolve them anyway. For TypeScript
                        // module imports, fall through and treat them as regular
                        // named imports so the source module's class info
                        // flows into `imported_classes` (for method dispatch on
                        // a `q: Query` typed local) and `cross_module_class_field_types`
                        // (for type inference on `someLocal.field` for-of
                        // iteration). Pre-fix, `import { type Query } from
                        // "../src"` made `q.forEach(...)` a silent no-op
                        // because Query was never registered with codegen —
                        // the spread call on the closure invoked a stub that
                        // returned undefined. ECS demo-simple repro.
                        let spec_type_only = named.is_type_only || whole_decl_type_only;
                        if spec_type_only && is_native {
                            // #1483: a type-only perry/ui widget import — e.g.
                            // `import { type Canvas as CanvasType }` — carries no
                            // value binding and is otherwise dropped here, but a
                            // `canvas: CanvasType` parameter still needs handle
                            // method dispatch. Record alias → canonical widget so
                            // fn_decl's param registration can resolve it.
                            if source == "perry/ui" {
                                let local = named.local.sym.to_string();
                                let imported = named
                                    .imported
                                    .as_ref()
                                    .map(|i| match i {
                                        ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                        ast::ModuleExportName::Str(s) => {
                                            s.value.as_str().unwrap_or("").to_string()
                                        }
                                    })
                                    .unwrap_or_else(|| local.clone());
                                if super::context::perry_ui_handle_widget(&imported) {
                                    ctx.ui_widget_type_aliases.insert(local, imported);
                                }
                            }
                            continue;
                        }
                        let local = named.local.sym.to_string();
                        let imported = named
                            .imported
                            .as_ref()
                            .map(|i| match i {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            })
                            .unwrap_or_else(|| local.clone());
                        if is_native {
                            // Register as native module function with the original method name
                            // e.g., import { v4 as uuid } from 'uuid' -> uuid maps to uuid.v4
                            ctx.register_native_module(
                                local.clone(),
                                source.clone(),
                                Some(imported.clone()),
                            );
                            // Auto-register parentPort from worker_threads as a native instance
                            // (it's a singleton, not created via `new`)
                            if source == "worker_threads" && imported == "parentPort" {
                                ctx.register_native_instance(
                                    local.clone(),
                                    "worker_threads".to_string(),
                                    "MessagePort".to_string(),
                                );
                            }
                        } else {
                            // Register as imported function (we assume all imports are functions for now)
                            ctx.register_imported_func(local.clone(), imported.clone());
                        }
                        specifiers.push(ImportSpecifier::Named { imported, local });
                    }
                    ast::ImportSpecifier::Default(default) => {
                        let local = default.local.sym.to_string();
                        if is_native {
                            // Default import of native module (e.g., import mysql from 'mysql2/promise')
                            // Default exports don't have a method name
                            ctx.register_native_module(local.clone(), source.clone(), None);
                        } else {
                            // Default import from JS module — register so calls resolve to
                            // ExternFuncRef. Use the LOCAL name as the original-name marker
                            // (identity registration) so the resulting ExternFuncRef carries a
                            // per-import-site UNIQUE name. Before this, every default import
                            // registered `(local, "default")`, so two `import X from "./a";
                            // import Y from "./b"` in the same file both lowered to
                            // `ExternFuncRef { name: "default" }` and the codegen's flat
                            // `import_function_prefixes` lookup keyed on "default" collided —
                            // whichever module's prefix landed last in the HashMap won, and
                            // both `X` and `Y` resolved to the SAME source module. Pino's
                            // CJS wrap exposed this: `const { SORTING_ORDER } =
                            // require('./lib/constants')` and `const { ... } =
                            // require('./lib/tools')` get hoisted as
                            // `import _req_9 from './lib/constants'; import _req_10 from
                            // './lib/tools'`, and pino.js's IIFE call `require('./lib/constants')`
                            // returned tools' exports, throwing
                            // `TypeError: Cannot read properties of undefined (reading 'ASC')`.
                            //
                            // The local name is unique per import site, so the resulting
                            // `ExternFuncRef { name: <local> }` looks up correctly against the
                            // CLI's `import_function_prefixes` (which already inserts both
                            // `exported_name="default"` AND `local_name=<unique>`). The CLI's
                            // companion insert at `crates/perry/src/commands/compile.rs`
                            // (right after the `import_function_prefixes` insert pair for
                            // Default specifiers) places `local_name → "default"` into
                            // `import_function_origin_names`, so the codegen's symbol
                            // construction at `lower_call.rs` / `expr.rs` builds
                            // `perry_fn_<src>__default` — matching what the origin module
                            // actually emits. Closes #901.
                            ctx.register_imported_func(local.clone(), local.clone());
                        }
                        specifiers.push(ImportSpecifier::Default { local });
                    }
                    ast::ImportSpecifier::Namespace(ns) => {
                        let local = ns.local.sym.to_string();
                        if is_native {
                            // Namespace import of native module (e.g., import * as mysql from 'mysql2')
                            // Methods are called via the namespace, so no specific method name
                            ctx.register_native_module(local.clone(), source.clone(), None);
                            // Also register as builtin module alias so method-level
                            // recognition works (child_process, fs, os, etc.)
                            ctx.register_builtin_module_alias(local.clone(), source.clone());
                        } else {
                            // Namespace import from JS module - register so calls resolve to ExternFuncRef
                            ctx.register_imported_func(local.clone(), local.clone());
                        }
                        specifiers.push(ImportSpecifier::Namespace { local });
                    }
                }
            }

            // Determine module kind based on the source and whether it's native
            let module_kind = if is_native {
                ModuleKind::NativeRust
            } else {
                // Default to NativeCompiled - the compiler driver will update this
                // based on file resolution
                ModuleKind::NativeCompiled
            };

            module.imports.push(Import {
                source,
                specifiers,
                is_native,
                module_kind,
                resolved_path: None, // Will be set by compiler driver during module resolution
                type_only: whole_decl_type_only,
                is_dynamic: false,
                is_dynamic_target: false,
            });
        }
        ast::ModuleDecl::ExportDecl(export) => {
            match &export.decl {
                ast::Decl::Fn(fn_decl) => {
                    // Skip overload signatures (no body) — they share the same func_id
                    // as the implementation. Pushing them to module.functions would cause
                    // codegen to compile the empty-body overload and skip the real implementation.
                    if fn_decl.function.body.is_none() {
                        return Ok(());
                    }
                    let mut func = lower_fn_decl(ctx, fn_decl)?;
                    func.is_exported = true;
                    let func_name = func.name.clone();
                    let func_id = func.id;
                    // Register return type for call-site inference
                    if !matches!(func.return_type, Type::Any) {
                        ctx.register_func_return_type(func_name.clone(), func.return_type.clone());
                    }
                    // If the declared return type maps to a native instance
                    // (e.g. `function openSocket(): Socket { ... }`), register
                    // the function as a factory so call sites can pick up
                    // the instance class — see lookup_func_return_native_instance.
                    if let Some((module, class)) =
                        native_instance_from_return_type(&func.return_type)
                    {
                        ctx.func_return_native_instances.push((
                            func_name.clone(),
                            module.to_string(),
                            class.to_string(),
                        ));
                    }
                    // Store parameter defaults for call-site resolution
                    let defaults: Vec<Option<Expr>> =
                        func.params.iter().map(|p| p.default.clone()).collect();
                    let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                    let rest_idx = func.params.iter().position(|p| p.is_rest);
                    let has_synth_args = func
                        .params
                        .last()
                        .is_some_and(|p| p.is_rest && p.name == "arguments");
                    ctx.func_defaults.push((
                        func.id,
                        defaults,
                        param_ids,
                        rest_idx,
                        has_synth_args,
                    ));
                    module.functions.push(func);
                    // Track in exports
                    module.exports.push(Export::Named {
                        local: func_name.clone(),
                        exported: func_name.clone(),
                    });
                    // Track exported function for cross-module value passing
                    module.exported_functions.push((func_name, func_id));
                }
                ast::Decl::Var(var_decl) => {
                    // Handle exported variables
                    for decl in &var_decl.decls {
                        let name = get_binding_name(&decl.name)?;
                        let ty = extract_binding_type(&decl.name);
                        if let Some(init) = &decl.init {
                            // Check if this is a native class instantiation and register it.
                            // Mirrors the destructuring.rs path: first try the
                            // general `lookup_native_module` (covers any class
                            // imported from a known native module), then fall
                            // back to a small hardcoded map for global-style
                            // names. Pool/Client/MongoClient are intentionally
                            // NOT in the fallback — those names collide with
                            // user classes and TS-source npm packages (e.g.
                            // `@perryts/mysql` exports its own `Pool`); the
                            // legitimate `import { Pool } from "pg"` flow is
                            // caught by the general lookup above. (Issue #536.)
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name = class_ident.sym.as_ref();
                                    // If the user has declared a class with this
                                    // name in the current module, it shadows the
                                    // hardcoded library-name fallback below — the
                                    // user-defined class wins. Without this gate
                                    // a user's `class Big { f0=0; ... }` was
                                    // routed through `big.js`'s handle-based
                                    // method dispatch (returning 0 for every
                                    // unknown property), so reads of any field
                                    // returned 0. Same shadowing logic applies
                                    // to `Decimal`, `BigNumber`, etc.
                                    let user_class_defined = module
                                        .classes
                                        .iter()
                                        .any(|c| c.name == class_name)
                                        || ctx.pending_classes.iter().any(|c| c.name == class_name);
                                    let module_name: Option<String> = if let Some((m, _)) =
                                        ctx.lookup_native_module(class_name)
                                    {
                                        Some(m.to_string())
                                    } else if user_class_defined {
                                        None
                                    } else {
                                        match class_name {
                                            "EventEmitter" => Some("events".to_string()),
                                            "AsyncLocalStorage" => Some("async_hooks".to_string()),
                                            "AsyncResource" => Some("async_hooks".to_string()),
                                            "WebSocket" | "WebSocketServer" => {
                                                Some("ws".to_string())
                                            }
                                            "Redis" => Some("ioredis".to_string()),
                                            "LRUCache" => Some("lru-cache".to_string()),
                                            "Command" => Some("commander".to_string()),
                                            "Big" => Some("big.js".to_string()),
                                            "Decimal" => Some("decimal.js".to_string()),
                                            "BigNumber" => Some("bignumber.js".to_string()),
                                            _ => None,
                                        }
                                    };
                                    // Issue #848: StringDecoder runs entirely through
                                    // HANDLE_METHOD_DISPATCH / HANDLE_PROPERTY_DISPATCH
                                    // — registering it as a typed native instance would
                                    // re-route `d.write` (property read) through the
                                    // NativeMethodCall-with-empty-args path that
                                    // pre-invokes the FFI as a getter, so
                                    // `typeof d.write === "function"` would silently
                                    // become `"number"` (the empty-string write return,
                                    // misclassified). Skipping the registration lets
                                    // the regular PropertyGet path fall into
                                    // HANDLE_PROPERTY_DISPATCH which returns the bound-
                                    // method closure built by `js_class_method_bind` —
                                    // `typeof` reads `"function"`, and the eventual
                                    // call routes through HANDLE_METHOD_DISPATCH back
                                    // to the same `dispatch_string_decoder` impl.
                                    let module_name = match (class_name, module_name.as_deref()) {
                                        ("StringDecoder", Some("string_decoder")) => None,
                                        _ => module_name,
                                    };
                                    if let Some(native_module) = module_name {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            native_module,
                                            class_name.to_string(),
                                        );
                                    }
                                }
                            }

                            // Check if this is an awaited native class instantiation (e.g., await new Redis())
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::New(new_expr) = await_expr.arg.as_ref() {
                                    if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref()
                                    {
                                        let class_name = class_ident.sym.as_ref();
                                        // Same user-class shadowing rule as the
                                        // non-await new-expr path above.
                                        let user_class_defined =
                                            module.classes.iter().any(|c| c.name == class_name)
                                                || ctx
                                                    .pending_classes
                                                    .iter()
                                                    .any(|c| c.name == class_name);
                                        let module_name: Option<String> = if let Some((m, _)) =
                                            ctx.lookup_native_module(class_name)
                                        {
                                            Some(m.to_string())
                                        } else if user_class_defined {
                                            None
                                        } else {
                                            match class_name {
                                                "EventEmitter" => Some("events".to_string()),
                                                "AsyncLocalStorage" => {
                                                    Some("async_hooks".to_string())
                                                }
                                                "AsyncResource" => Some("async_hooks".to_string()),
                                                "WebSocket" | "WebSocketServer" => {
                                                    Some("ws".to_string())
                                                }
                                                "Redis" => Some("ioredis".to_string()),
                                                "LRUCache" => Some("lru-cache".to_string()),
                                                "Command" => Some("commander".to_string()),
                                                "Big" => Some("big.js".to_string()),
                                                "Decimal" => Some("decimal.js".to_string()),
                                                "BigNumber" => Some("bignumber.js".to_string()),
                                                _ => None,
                                            }
                                        };
                                        // Issue #848: StringDecoder runs entirely through
                                        // HANDLE_*_DISPATCH (see the gate on the sync path
                                        // above for the full rationale). Defensive mirror
                                        // on this awaited-new branch so the same skip
                                        // applies to `await new StringDecoder(...)` —
                                        // which is unusual but legal TS.
                                        let module_name = match (class_name, module_name.as_deref())
                                        {
                                            ("StringDecoder", Some("string_decoder")) => None,
                                            _ => module_name,
                                        };
                                        if let Some(native_module) = module_name {
                                            ctx.register_native_instance(
                                                name.clone(),
                                                native_module,
                                                class_name.to_string(),
                                            );
                                        }
                                    }
                                }
                            }

                            // Check if this is a native module factory function call (e.g., mysql.createPool())
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let ast::Callee::Expr(callee) = &call_expr.callee {
                                    if let ast::Expr::Member(member) = callee.as_ref() {
                                        if let ast::Expr::Ident(obj_ident) = member.obj.as_ref() {
                                            let obj_name = obj_ident.sym.as_ref();
                                            // Check if it's a known native module
                                            // Clone module_name to avoid borrow conflict with ctx mutation below
                                            let native_mod = ctx
                                                .lookup_native_module(obj_name)
                                                .map(|(m, _)| m.to_string());
                                            if let Some(module_name_owned) = native_mod {
                                                if let ast::MemberProp::Ident(method_ident) =
                                                    &member.prop
                                                {
                                                    let method_name = method_ident.sym.as_ref();
                                                    // Map factory functions to their class names
                                                    let class_name = match (
                                                        module_name_owned.as_str(),
                                                        method_name,
                                                    ) {
                                                        (
                                                            "mysql2" | "mysql2/promise",
                                                            "createPool",
                                                        ) => Some("Pool"),
                                                        (
                                                            "mysql2" | "mysql2/promise",
                                                            "createConnection",
                                                        ) => Some("Connection"),
                                                        ("pg", "connect") => Some("Client"),
                                                        ("http" | "https", "request" | "get") => {
                                                            Some("ClientRequest")
                                                        }
                                                        // net.createConnection(host, port) returns a Socket handle.
                                                        // Without registering this, subsequent `sock.write/on/end/destroy`
                                                        // calls fall through to dynamic dispatch and never reach
                                                        // the `js_net_socket_*` FFI functions.
                                                        ("net", "createConnection") => {
                                                            Some("Socket")
                                                        }
                                                        // node-cron's `cron.schedule(expr, cb)` returns a job
                                                        // handle whose `start()`/`stop()`/`isRunning()` etc.
                                                        // dispatch via the ("node-cron", true, METHOD) entries
                                                        // in expr.rs's native_module dispatch table. Without
                                                        // registering the handle as a "CronJob" native instance,
                                                        // `job.stop()` falls through to dynamic dispatch and the
                                                        // stop never reaches js_cron_job_stop.
                                                        ("node-cron", "schedule") => {
                                                            Some("CronJob")
                                                        }
                                                        // node:http server (issue #577) — `http.createServer(...)`
                                                        // returns an HttpServer handle whose `.listen(...)` /
                                                        // `.close()` / `.address()` / `.on(...)` dispatches via
                                                        // the ("http", true, *) class_filter=Some("HttpServer")
                                                        // entries in NATIVE_MODULE_TABLE.
                                                        ("http", "createServer") => {
                                                            Some("HttpServer")
                                                        }
                                                        ("https", "createServer") => {
                                                            Some("HttpsServer")
                                                        }
                                                        ("http2", "createSecureServer") => {
                                                            Some("Http2SecureServer")
                                                        }
                                                        ("async_hooks", "createHook") => {
                                                            Some("AsyncHook")
                                                        }
                                                        _ => None,
                                                    };
                                                    if let Some(class_name) = class_name {
                                                        ctx.register_native_instance(
                                                            name.clone(),
                                                            module_name_owned.clone(),
                                                            class_name.to_string(),
                                                        );
                                                        // Also register as module-level native instance so it survives scope exits.
                                                        // Without this, pool = mysql.createPool() at module top level loses
                                                        // its native tracking when function scopes are entered/exited,
                                                        // causing pool.query() inside functions to miss the Pool dispatch.
                                                        ctx.module_native_instances.push((
                                                            name.clone(),
                                                            module_name_owned,
                                                            class_name.to_string(),
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Check if this is a direct call to a default import from a native module
                                    // e.g., Fastify() where Fastify is imported from 'fastify'
                                    if let ast::Expr::Ident(func_ident) = callee.as_ref() {
                                        let func_name = func_ident.sym.as_ref();
                                        // Check if this is a default import from a native module
                                        if let Some((module_name, None)) =
                                            ctx.lookup_native_module(func_name)
                                        {
                                            // Register as native instance - the "class" is the module name for default exports
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module_name.to_string(),
                                                "App".to_string(),
                                            );
                                        }
                                        // Check if this is a named import that returns a handle (e.g., State from perry/ui)
                                        if let Some((module_name, Some(method_name))) =
                                            ctx.lookup_native_module(func_name)
                                        {
                                            if module_name == "perry/ui" {
                                                match method_name {
                                                    "Canvas" | "State" | "Sheet" | "Toolbar"
                                                    | "Window" | "LazyVStack"
                                                    | "NavigationStack" | "Picker" | "Table"
                                                    | "TabBar" => {
                                                        ctx.register_native_instance(
                                                            name.clone(),
                                                            module_name.to_string(),
                                                            method_name.to_string(),
                                                        );
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        // node:http server (issue #577) — named-import factory:
                                        // `import { createServer } from "node:http"; const s = createServer(...)`.
                                        // Resolve module + method up front to drop the immutable borrow
                                        // on `ctx` before we mutate via register_native_instance + push.
                                        let factory_class: Option<&'static str> = ctx
                                            .lookup_native_module(func_name)
                                            .and_then(|(m, method)| match (m, method) {
                                                ("http", Some("createServer")) => {
                                                    Some("HttpServer")
                                                }
                                                ("https", Some("createServer")) => {
                                                    Some("HttpsServer")
                                                }
                                                ("http2", Some("createSecureServer")) => {
                                                    Some("Http2SecureServer")
                                                }
                                                ("async_hooks", Some("createHook")) => {
                                                    Some("AsyncHook")
                                                }
                                                _ => None,
                                            });
                                        if let Some(class_name) = factory_class {
                                            // Module name comes from `lookup_native_module` —
                                            // re-look it up after the borrow ends.
                                            let module = ctx
                                                .lookup_native_module(func_name)
                                                .map(|(m, _)| m.to_string())
                                                .unwrap_or_default();
                                            ctx.register_native_instance(
                                                name.clone(),
                                                module.clone(),
                                                class_name.to_string(),
                                            );
                                            ctx.module_native_instances.push((
                                                name.clone(),
                                                module,
                                                class_name.to_string(),
                                            ));
                                        }
                                    }
                                }
                            }

                            // Check if this is an awaited factory call (e.g., const client = await MongoClient.connect(uri))
                            if let ast::Expr::Await(await_expr) = init.as_ref() {
                                if let ast::Expr::Call(call_expr) = await_expr.arg.as_ref() {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref()
                                            {
                                                let obj_name = obj_ident.sym.as_ref();
                                                if let Some((module_name, _)) =
                                                    ctx.lookup_native_module(obj_name)
                                                {
                                                    if let ast::MemberProp::Ident(method_ident) =
                                                        &member.prop
                                                    {
                                                        let class_name = match (
                                                            module_name,
                                                            method_ident.sym.as_ref(),
                                                        ) {
                                                            ("mongodb", "connect") => {
                                                                Some("MongoClient")
                                                            }
                                                            (
                                                                "mysql2" | "mysql2/promise",
                                                                "createPool",
                                                            ) => Some("Pool"),
                                                            (
                                                                "mysql2" | "mysql2/promise",
                                                                "createConnection",
                                                            ) => Some("Connection"),
                                                            ("pg", "connect") => Some("Client"),
                                                            (
                                                                "http" | "https",
                                                                "request" | "get",
                                                            ) => Some("ClientRequest"),
                                                            (
                                                                "axios",
                                                                "get" | "post" | "put" | "delete"
                                                                | "patch" | "request",
                                                            ) => Some("Response"),
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = class_name {
                                                            ctx.register_native_instance(
                                                                name.clone(),
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
                            }

                            // Check if this is a `new NativeClass(...)` expression
                            // e.g., const db = new Database('mango.db') where Database is from better-sqlite3
                            if let ast::Expr::New(new_expr) = init.as_ref() {
                                if let ast::Expr::Ident(class_ident) = new_expr.callee.as_ref() {
                                    let class_name_str = class_ident.sym.as_ref();
                                    // Check if this class comes from a native module import
                                    let native_info = ctx
                                        .lookup_native_module(class_name_str)
                                        .map(|(m, _)| m.to_string());
                                    if let Some(module_name) = native_info {
                                        ctx.register_native_instance(
                                            name.clone(),
                                            module_name.clone(),
                                            class_name_str.to_string(),
                                        );
                                        ctx.module_native_instances.push((
                                            name.clone(),
                                            module_name,
                                            class_name_str.to_string(),
                                        ));
                                    }
                                } else if let ast::Expr::Member(member) = new_expr.callee.as_ref() {
                                    if let (
                                        ast::Expr::Ident(module_ident),
                                        ast::MemberProp::Ident(class_ident),
                                    ) = (member.obj.as_ref(), &member.prop)
                                    {
                                        let module_alias = module_ident.sym.as_ref();
                                        if let Some(module_name) = ctx
                                            .lookup_native_module(module_alias)
                                            .map(|(m, _)| m.to_string())
                                        {
                                            let class_name_str = class_ident.sym.as_ref();
                                            let is_known_native_class = matches!(
                                                (module_name.as_str(), class_name_str),
                                                (
                                                    "async_hooks",
                                                    "AsyncLocalStorage" | "AsyncResource"
                                                )
                                            );
                                            if is_known_native_class {
                                                ctx.register_native_instance(
                                                    name.clone(),
                                                    module_name.clone(),
                                                    class_name_str.to_string(),
                                                );
                                                ctx.module_native_instances.push((
                                                    name.clone(),
                                                    module_name,
                                                    class_name_str.to_string(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is a method call on a registered native instance (chaining).
                            // e.g., const db = client.db(name) where client is a mongodb native instance.
                            {
                                // Unwrap await if present
                                let actual_init =
                                    if let ast::Expr::Await(await_expr) = init.as_ref() {
                                        await_expr.arg.as_ref()
                                    } else {
                                        init.as_ref()
                                    };
                                if let ast::Expr::Call(call_expr) = actual_init {
                                    if let ast::Callee::Expr(callee) = &call_expr.callee {
                                        if let ast::Expr::Member(member) = callee.as_ref() {
                                            if let ast::Expr::Ident(obj_ident) = member.obj.as_ref()
                                            {
                                                let obj_name = obj_ident.sym.to_string();
                                                if let Some((module_name, _class)) = ctx
                                                    .lookup_native_instance(&obj_name)
                                                    .map(|(m, c)| (m.to_string(), c.to_string()))
                                                {
                                                    if let ast::MemberProp::Ident(method_ident) =
                                                        &member.prop
                                                    {
                                                        let method_name = method_ident.sym.as_ref();
                                                        // Determine if the method returns a handle (another native instance)
                                                        let returns_handle = match (
                                                            module_name.as_str(),
                                                            method_name,
                                                        ) {
                                                            ("mongodb", "db") => Some("Database"),
                                                            ("mongodb", "collection") => {
                                                                Some("Collection")
                                                            }
                                                            (
                                                                "mysql2" | "mysql2/promise",
                                                                "getConnection",
                                                            ) => Some("PoolConnection"),
                                                            ("better-sqlite3", "prepare") => {
                                                                Some("Statement")
                                                            }
                                                            _ => None,
                                                        };
                                                        if let Some(class_name) = returns_handle {
                                                            ctx.register_native_instance(
                                                                name.clone(),
                                                                module_name,
                                                                class_name.to_string(),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if this is an arrow function with a native return type
                            // e.g., export const getRedis = async (): Promise<Redis> => { ... }
                            if let ast::Expr::Arrow(arrow) = init.as_ref() {
                                if let Some(ref rt) = arrow.return_type {
                                    let return_type =
                                        extract_ts_type_with_ctx(&rt.type_ann, Some(ctx));
                                    // Unwrap Promise<T> for async functions
                                    let check_type = match &return_type {
                                        Type::Generic { base, type_args } if base == "Promise" => {
                                            type_args.first().unwrap_or(&return_type)
                                        }
                                        Type::Promise(inner) => inner.as_ref(),
                                        other => other,
                                    };
                                    if let Type::Named(type_name) = check_type {
                                        let module_info = match type_name.as_str() {
                                            "Redis" => Some(("ioredis", "Redis")),
                                            "EventEmitter" => Some(("events", "EventEmitter")),
                                            "Pool" => Some(("mysql2/promise", "Pool")),
                                            "PoolConnection" => {
                                                Some(("mysql2/promise", "PoolConnection"))
                                            }
                                            "WebSocket" | "WebSocketServer" => {
                                                Some(("ws", type_name.as_str()))
                                            }
                                            // perry-stdlib net.Socket: lets library wrappers like
                                            //   export function openSocket(host, port): Socket { ... }
                                            // propagate native-instance tagging to callers, so
                                            //   const sock = openSocket(...);
                                            //   sock.on(...);   // dispatches to js_net_socket_on
                                            // works without ceremony.
                                            "Socket" => Some(("net", "Socket")),
                                            _ => {
                                                // Also check dotted names (e.g., mysql.Pool)
                                                if let Some(dot_pos) = type_name.find('.') {
                                                    let module_alias = &type_name[..dot_pos];
                                                    let class_name = &type_name[dot_pos + 1..];
                                                    if let Some((module_name, _)) =
                                                        ctx.lookup_native_module(module_alias)
                                                    {
                                                        Some((module_name, class_name))
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            }
                                        };
                                        if let Some((module, class)) = module_info {
                                            ctx.func_return_native_instances.push((
                                                name.clone(),
                                                module.to_string(),
                                                class.to_string(),
                                            ));
                                        }
                                    }
                                }
                            }

                            // Track exported values that need cross-module access.
                            // Any exported const/let with an initializer needs a global data slot
                            // so that importing modules can read its value at runtime.
                            // Previously this only matched Object/Call/Array/New/Arrow expressions,
                            // which caused exported string, number, bigint, and boolean constants
                            // to be undefined when imported by other modules.
                            let needs_export_global = true;

                            // Check if this is a Widget({...}) call from perry/widget
                            if let ast::Expr::Call(call_expr) = init.as_ref() {
                                if let Some(widget_decl) = try_lower_widget_decl(ctx, call_expr) {
                                    module.widgets.push(widget_decl);
                                    continue;
                                }
                            }

                            let expr = lower_expr(ctx, init)?;
                            let id = if ctx.pre_registered_module_vars.remove(&name) {
                                let id = ctx.lookup_local(&name).unwrap();
                                if let Some((_, _, existing_ty)) =
                                    ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name)
                                {
                                    *existing_ty = ty.clone();
                                }
                                id
                            } else {
                                ctx.define_local(name.clone(), ty.clone())
                            };
                            module.init.push(Stmt::Let {
                                id,
                                name: name.clone(),
                                ty,
                                mutable: matches!(
                                    var_decl.kind,
                                    ast::VarDeclKind::Let | ast::VarDeclKind::Var
                                ),
                                init: Some(expr),
                            });
                            module.exports.push(Export::Named {
                                local: name.clone(),
                                exported: name.clone(),
                            });

                            // Register exported values that need cross-module globals
                            if needs_export_global {
                                module.exported_objects.push(name.clone());
                            }

                            // Handle identifier aliases: export const foo = existingVar;
                            if let ast::Expr::Ident(ident) = init.as_ref() {
                                let ref_name = ident.sym.to_string();
                                if let Some(func_id) = ctx.lookup_func(&ref_name) {
                                    // Function alias - add to exported_functions
                                    module.exported_functions.push((name, func_id));
                                } else {
                                    // Non-function alias (e.g., export const alias = someObject)
                                    // Needs its own export global for cross-module access
                                    module.exported_objects.push(name.clone());
                                }
                            }
                        }
                    }
                }
                ast::Decl::Class(class_decl) => {
                    let class = lower_class_decl(ctx, class_decl, true)?;
                    let class_name = class.name.clone();
                    // Issue #711: dynamic parent-class registration at the
                    // source position (see non-export class arm below for
                    // rationale).
                    if let Some(extends_expr) = &class.extends_expr {
                        module
                            .init
                            .push(Stmt::Expr(Expr::RegisterClassParentDynamic {
                                class_name: class_name.clone(),
                                parent_expr: extends_expr.clone(),
                            }));
                    }
                    // Inject static-field-init statements in source order
                    // (see non-export class arm below for rationale).
                    for sf in &class.static_fields {
                        if let Some(init) = &sf.init {
                            // Computed-key static fields (`static [sym] = v`)
                            // emit a runtime-register call instead of a
                            // string-keyed StaticFieldSet. Refs #420.
                            if let Some(key) = sf.key_expr.as_ref() {
                                module.init.push(Stmt::Expr(Expr::ClassStaticSymbolSet {
                                    class_name: class_name.clone(),
                                    key: Box::new(key.clone()),
                                    value: Box::new(init.clone()),
                                }));
                            } else {
                                module.init.push(Stmt::Expr(Expr::StaticFieldSet {
                                    class_name: class_name.clone(),
                                    field_name: sf.name.clone(),
                                    value: Box::new(init.clone()),
                                }));
                            }
                        }
                    }
                    append_legacy_decorator_init_for_class(ctx, &mut module.init, &class);
                    push_class_dedup(module, class);
                    module.exports.push(Export::Named {
                        local: class_name.clone(),
                        exported: class_name,
                    });
                }
                ast::Decl::TsEnum(enum_decl) => {
                    let en = lower_enum_decl(ctx, enum_decl, true)?;
                    let enum_name = en.name.clone();
                    module.enums.push(en);
                    module.exported_objects.push(enum_name.clone());
                    module.exports.push(Export::Named {
                        local: enum_name.clone(),
                        exported: enum_name,
                    });
                }
                ast::Decl::TsInterface(iface_decl) => {
                    let iface = lower_interface_decl(ctx, iface_decl, true)?;
                    let iface_name = iface.name.clone();
                    module.interfaces.push(iface);
                    module.exports.push(Export::Named {
                        local: iface_name.clone(),
                        exported: iface_name,
                    });
                }
                ast::Decl::TsTypeAlias(alias_decl) => {
                    let alias = lower_type_alias_decl(ctx, alias_decl, true)?;
                    let alias_name = alias.name.clone();
                    module.type_aliases.push(alias);
                    module.exports.push(Export::Named {
                        local: alias_name.clone(),
                        exported: alias_name,
                    });
                }
                ast::Decl::TsModule(ts_module) => {
                    // export namespace X { ... } — lower as a synthetic class with static members
                    if !ts_module.declare {
                        if let Some(ref body) = ts_module.body {
                            let ns_name = match &ts_module.id {
                                ast::TsModuleName::Ident(ident) => ident.sym.to_string(),
                                ast::TsModuleName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            let class =
                                lower_namespace_as_class(ctx, module, &ns_name, body, true)?;
                            let class_name = class.name.clone();
                            push_class_dedup(module, class);
                            module.exports.push(Export::Named {
                                local: class_name.clone(),
                                exported: class_name,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        ast::ModuleDecl::ExportNamed(export_named) => {
            // Skip type-only exports (export type { ... }) - they have no runtime value
            if export_named.type_only {
                return Ok(());
            }
            // export { foo, bar as baz }
            // export { foo } from "source"
            if let Some(ref src) = export_named.src {
                // Re-export from another module
                let source = src.value.as_str().unwrap_or("").to_string();
                for spec in &export_named.specifiers {
                    match spec {
                        ast::ExportSpecifier::Named(named) => {
                            // Skip individual type-only specifiers (export { type Foo, Bar })
                            if named.is_type_only {
                                continue;
                            }
                            let local = match &named.orig {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            let exported = named
                                .exported
                                .as_ref()
                                .map(|e| match e {
                                    ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                    ast::ModuleExportName::Str(s) => {
                                        s.value.as_str().unwrap_or("").to_string()
                                    }
                                })
                                .unwrap_or_else(|| local.clone());
                            module.exports.push(Export::ReExport {
                                source: source.clone(),
                                imported: local,
                                exported,
                            });
                        }
                        ast::ExportSpecifier::Namespace(ns) => {
                            // `export * as Foo from "./Foo"` — closes #310. Pre-fix
                            // SWC's `ExportSpecifier::Namespace` was silently dropped
                            // here because the arm only matched `Named`. The
                            // re-exported file then never entered the module graph,
                            // and every `<name>.<member>` access in consumer code
                            // lowered to 0 (the unknown-identifier sentinel).
                            let name = match &ns.name {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            };
                            module.exports.push(Export::NamespaceReExport {
                                source: source.clone(),
                                name,
                            });
                        }
                        // `export v from 'mod'` — TC39 stage-1, never standardised.
                        // Not emitted by tsc/swc TS output, so silently ignore.
                        ast::ExportSpecifier::Default(_) => {}
                    }
                }
            } else {
                // Local export: export { foo, bar as baz }
                for spec in &export_named.specifiers {
                    if let ast::ExportSpecifier::Named(named) = spec {
                        // Skip individual type-only specifiers (export { type Foo, Bar })
                        if named.is_type_only {
                            continue;
                        }
                        let local = match &named.orig {
                            ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                            ast::ModuleExportName::Str(s) => {
                                s.value.as_str().unwrap_or("").to_string()
                            }
                        };
                        let exported = named
                            .exported
                            .as_ref()
                            .map(|e| match e {
                                ast::ModuleExportName::Ident(id) => id.sym.to_string(),
                                ast::ModuleExportName::Str(s) => {
                                    s.value.as_str().unwrap_or("").to_string()
                                }
                            })
                            .unwrap_or_else(|| local.clone());
                        module.exports.push(Export::Named {
                            local: local.clone(),
                            exported: exported.clone(),
                        });

                        // Fix #482 (v0.5.577): when `class Foo {}` is followed by
                        // `export { Foo }` in a separate clause, the class was
                        // lowered with `is_exported = false` (because the decl
                        // wasn't a syntactic `export class Foo {}`). The
                        // CLI-driver's `exported_classes` lookup
                        // (`crates/perry/src/commands/compile.rs:1684`) filters
                        // on `class.is_exported`, so the class never reached
                        // the importer's `imported_classes` registry — `new
                        // Foo()` from another module fell through to the
                        // empty-object placeholder. Flip the class's
                        // is_exported bit when an export clause names it.
                        for class in module.classes.iter_mut() {
                            if class.name == local {
                                class.is_exported = true;
                                break;
                            }
                        }

                        // Fix #588: parallel of the class arm above for function
                        // decls. `function drizzle() {}` followed by `export {
                        // drizzle }` was lowered with `is_exported = false` and
                        // never landed in `module.exported_functions`. The CLI
                        // driver's `exported_func_param_counts` lookup at
                        // compile.rs:1907 filters on `func.is_exported`, and
                        // its fallback at compile.rs:1921 iterates only
                        // `exported_functions` — both miss this function.
                        // Importers then fall back to `args.len()` in
                        // `lower_call.rs:767` and declare the cross-module
                        // function with the wrong arity. The ABI mismatch
                        // causes trailing default-defaulted params to read
                        // stale register state (typically a small
                        // f64::from_bits(N) value, e.g. 1.26e-321), so the
                        // body's `if (p === undefined) p = default` desugar
                        // never fires.
                        if let Some(func_id) = ctx.lookup_func(&local) {
                            for func in module.functions.iter_mut() {
                                if func.id == func_id {
                                    func.is_exported = true;
                                    break;
                                }
                            }
                            if !module
                                .exported_functions
                                .iter()
                                .any(|(n, _)| n == &exported)
                            {
                                module.exported_functions.push((exported.clone(), func_id));
                            }
                        }

                        // Check if the variable is a closure or other exportable object
                        // by looking through init statements. For #460: also catch let
                        // bindings whose init is a function-reference value
                        // (`const _await = core.deferredAwait`) — without this branch
                        // the renamed export `_await as await` produces no backing
                        // global / getter at all and `_perry_fn_<mod>__await` link-fails.
                        for stmt in &module.init {
                            if let Stmt::Let {
                                name,
                                init: Some(init_expr),
                                ..
                            } = stmt
                            {
                                if name == &local {
                                    let is_exportable = matches!(
                                        init_expr,
                                        Expr::Closure { .. }
                                            | Expr::Object(_)
                                            | Expr::Array(_)
                                            | Expr::Call { .. }
                                            | Expr::New { .. }
                                            | Expr::JsNew { .. }
                                            | Expr::LocalGet(_)
                                            | Expr::FuncRef(_)
                                            | Expr::ExternFuncRef { .. }
                                            | Expr::PropertyGet { .. }
                                            // #421 fix (v0.5.574): primitive literals must
                                            // also flow through `exported_objects` so the
                                            // importing module's `imported_vars` set picks
                                            // them up — without this, `var X = "literal";
                                            // export { X };` (the shape hono / drizzle /
                                            // any prebundled JS uses for string / number
                                            // constants) gets imported as a closure-pointer
                                            // wrapper instead of the actual value, and
                                            // `typeof X` returns "function" + `X.toString`
                                            // prints `[object Object]`.
                                            | Expr::String(_)
                                            | Expr::Number(_)
                                            | Expr::Bool(_)
                                            | Expr::BigInt(_)
                                            | Expr::Null
                                            | Expr::Undefined
                                            // Refs #420 (drizzle): `const entityKind = Symbol.for(...)`
                                            // followed by `export { entityKind }` must register the
                                            // local as an exported variable so importing modules
                                            // pick it up via `imported_vars` (and route through the
                                            // getter, not as a closure pointer).
                                            | Expr::SymbolFor(_)
                                            // Issue #923: `const pool = mysql.createPool(...)`
                                            // followed by `export { pool }` lowers the init to
                                            // `Expr::NativeMethodCall` (not `Expr::Call`) because
                                            // `mysql` is a registered native-module alias and the
                                            // factory call resolves to the stdlib FFI dispatch.
                                            // Without this branch, `pool` never lands in
                                            // `exported_objects`, the producer-side getter
                                            // `perry_fn_<src>__pool` is never emitted, and the
                                            // consumer-side `ExternFuncRef { name: "pool" }` falls
                                            // through to the closure-wrapper path
                                            // (`__perry_wrap_perry_fn_<src>__pool`) which #836's
                                            // Sub-bug B emits as a no-op returning undefined. End
                                            // result: link succeeds but `typeof pool === "function"`
                                            // and `pool.execute(...)` segfaults. Mirroring the
                                            // inline-export shape (`export const pool = ...` at
                                            // line ~5087) makes both export forms equivalent.
                                            //
                                            // Covers every stdlib factory pattern: `mysql2/promise`
                                            // `createPool` / `createConnection`, `net.createConnection`,
                                            // `http.createServer`, `pg.connect`, `ioredis`
                                            // constructors, `tls.connect`, and any other
                                            // factory the codegen already lowers to a
                                            // `NativeMethodCall` via lookup_native_module.
                                            | Expr::NativeMethodCall { .. }
                                    );
                                    if is_exportable {
                                        module.exported_objects.push(exported.clone());
                                        // Ensure the LOCAL name also surfaces as
                                        // exported — that's what gates the global
                                        // emission in codegen (`exported_var_names`
                                        // is built from `exported_objects`).
                                        // Without the local entry, `_await`'s id
                                        // is never registered in `module_globals`,
                                        // so even the local-name getter is missing
                                        // and call sites that resolve through the
                                        // local name fall through to undefined.
                                        if !module.exported_objects.contains(&local) {
                                            module.exported_objects.push(local.clone());
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        ast::ModuleDecl::ExportDefaultDecl(export_default) => {
            // export default function foo() {} or export default class Foo {}
            match &export_default.decl {
                ast::DefaultDecl::Fn(fn_expr) => {
                    if let Some(ref ident) = fn_expr.ident {
                        // Named function: `export default function foo() {}`.
                        //
                        // Pre-fix this branch only pushed an `Export::Named`
                        // entry and dropped the body — so the consumer's
                        // `import foo from "./mod"; foo()` resolved to
                        // `ExternFuncRef { name: "default" }` and link/runtime
                        // produced `undefined` because no
                        // `perry_fn_<src>__default` (and no
                        // `perry_fn_<src>__foo`) symbol was ever emitted.
                        //
                        // This was the root cause of the uuid `v4()` smoke-
                        // test failure: `rng.js` is exactly
                        // `export default function rng() { ... }`, so the
                        // imported `rng()` call in `v4.js` returned undefined
                        // and the downstream `.length` access threw.
                        //
                        // Mirror the `ExportDecl::Fn` flow so the function
                        // body is actually emitted under
                        // `perry_fn_<src>__<ident>`, then register the
                        // function under the exported name `"default"`.
                        // codegen's alias-emission pass (codegen.rs ~L2259)
                        // sees `exported_name != f.name` and synthesizes the
                        // `perry_fn_<src>__default` forwarder pointing at
                        // `perry_fn_<src>__<ident>`.
                        let func_name = ident.sym.to_string();
                        if fn_expr.function.body.is_some() {
                            let synth_fn_decl = ast::FnDecl {
                                ident: ident.clone(),
                                declare: false,
                                function: fn_expr.function.clone(),
                            };
                            let mut func = lower_fn_decl(ctx, &synth_fn_decl)?;
                            func.is_exported = true;
                            let func_id = func.id;
                            if !matches!(func.return_type, Type::Any) {
                                ctx.register_func_return_type(
                                    func_name.clone(),
                                    func.return_type.clone(),
                                );
                            }
                            if let Some((mod_name, class)) =
                                native_instance_from_return_type(&func.return_type)
                            {
                                ctx.func_return_native_instances.push((
                                    func_name.clone(),
                                    mod_name.to_string(),
                                    class.to_string(),
                                ));
                            }
                            let defaults: Vec<Option<Expr>> =
                                func.params.iter().map(|p| p.default.clone()).collect();
                            let param_ids: Vec<LocalId> =
                                func.params.iter().map(|p| p.id).collect();
                            let rest_idx = func.params.iter().position(|p| p.is_rest);
                            let has_synth_args = func
                                .params
                                .last()
                                .is_some_and(|p| p.is_rest && p.name == "arguments");
                            ctx.func_defaults.push((
                                func.id,
                                defaults,
                                param_ids,
                                rest_idx,
                                has_synth_args,
                            ));
                            module.functions.push(func);
                            // Register under both names: callable locally as
                            // `<ident>` (some modules also `export { foo }`
                            // or call themselves by name) AND as the
                            // module's `default` export so importers resolve
                            // through the wrapper.
                            module.exports.push(Export::Named {
                                local: func_name.clone(),
                                exported: "default".to_string(),
                            });
                            module
                                .exported_functions
                                .push(("default".to_string(), func_id));
                        } else {
                            // Body-less form (declaration-only) — keep
                            // historical behavior of only recording the
                            // export entry.
                            module.exports.push(Export::Named {
                                local: func_name,
                                exported: "default".to_string(),
                            });
                        }
                    } else if fn_expr.function.body.is_some() {
                        // Anonymous-default-function (zod / vitest blocker):
                        // `export default function () { ... }` arrives as a
                        // `DefaultDecl::Fn(FnExpr)` with `fn_expr.ident == None`.
                        // Pre-fix this branch dropped the body entirely —
                        // codegen never saw the function, so
                        // `perry_fn_<src>__default` was never emitted and
                        // consumers link-failed with `Undefined symbols:
                        // _perry_fn_<src>__default`. The
                        // `__perry_wrap_perry_fn_<src>__default` rename
                        // wrapper added in #837 had nothing to point at
                        // either.
                        //
                        // Synthesize an `FnDecl` with ident `default` so
                        // the HIR function name (and therefore the LLVM
                        // symbol) is `perry_fn_<src>__default`, matching
                        // what consumers ask for. Run the resulting decl
                        // through the same flow as `ExportDecl::Fn`: lower
                        // the body, mark `is_exported`, register defaults
                        // and `exported_functions` so codegen's wrapper-
                        // emission machinery picks it up.
                        //
                        // Scope-narrow: this only changes the
                        // `fn_expr.ident == None` branch. The named-default
                        // case above keeps its existing behavior pending a
                        // separate fix.
                        let synth_ident = ast::Ident::new(
                            "default".to_string().into(),
                            swc_common::DUMMY_SP,
                            Default::default(),
                        );
                        let synth_fn_decl = ast::FnDecl {
                            ident: synth_ident,
                            declare: false,
                            function: fn_expr.function.clone(),
                        };
                        let mut func = lower_fn_decl(ctx, &synth_fn_decl)?;
                        func.is_exported = true;
                        let func_id = func.id;
                        // Defaults registration mirrors the `ExportDecl::Fn`
                        // path so call sites that pad missing args still
                        // resolve user-written defaults.
                        let defaults: Vec<Option<Expr>> =
                            func.params.iter().map(|p| p.default.clone()).collect();
                        let param_ids: Vec<LocalId> = func.params.iter().map(|p| p.id).collect();
                        let rest_idx = func.params.iter().position(|p| p.is_rest);
                        let has_synth_args = func
                            .params
                            .last()
                            .is_some_and(|p| p.is_rest && p.name == "arguments");
                        ctx.func_defaults.push((
                            func.id,
                            defaults,
                            param_ids,
                            rest_idx,
                            has_synth_args,
                        ));
                        module.functions.push(func);
                        // Both the named export entry (so the importer's
                        // namespace populator sees `default`) and the
                        // `exported_functions` registry (so codegen's
                        // alias / wrapper emission treats it as a real
                        // exported function) are required.
                        module.exports.push(Export::Named {
                            local: "default".to_string(),
                            exported: "default".to_string(),
                        });
                        module
                            .exported_functions
                            .push(("default".to_string(), func_id));
                    }
                }
                ast::DefaultDecl::Class(class_expr) => {
                    if let Some(ref ident) = class_expr.ident {
                        let class_name = ident.sym.to_string();
                        module.exports.push(Export::Named {
                            local: class_name,
                            exported: "default".to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        ast::ModuleDecl::ExportAll(export_all) => {
            // export * from "source"
            let source = export_all.src.value.as_str().unwrap_or("").to_string();
            module.exports.push(Export::ExportAll { source });
        }
        ast::ModuleDecl::ExportDefaultExpr(export_default_expr) => {
            // export default <expr>
            let lowered = lower_expr(ctx, &export_default_expr.expr)?;

            // If the expression is a FuncRef, add to exported_functions for proper wrapper generation
            if let Expr::FuncRef(func_id) = &lowered {
                // Find the function and add as exported with name "default"
                let func_id = *func_id;
                module
                    .exported_functions
                    .push(("default".to_string(), func_id));
                // Also mark the function as exported
                for func in &mut module.functions {
                    if func.id == func_id {
                        func.is_exported = true;
                        break;
                    }
                }
                module.exports.push(Export::Named {
                    local: "default".to_string(),
                    exported: "default".to_string(),
                });
            } else if let Expr::ClassRef(class_name) = &lowered {
                // Issue #665: `export default <ClassName>` where the
                // identifier names a class declared in this module — the
                // shape cjs_wrap emits for `module.exports = ClassName`
                // (e.g. rate-limiter-flexible's `RateLimiterMemory`).
                // Mirror both the `ExportDefaultDecl::Class` path AND the
                // `export { X }` Fix-#482 path: register the class
                // binding as the default export AND flip the class's
                // `is_exported` bit so the CLI driver's
                // `exported_classes` lookup (which filters on
                // `class.is_exported`) carries the class into the
                // importer's `imported_classes` registry. Without this,
                // the importer hits the fallback branch below: a
                // synthetic `default` Any-typed local whose value
                // happens to be the class but whose class identity HIR
                // lost — `new (default-import)(...)` then falls through
                // to the empty-object placeholder with every method
                // undefined.
                module.exports.push(Export::Named {
                    local: class_name.clone(),
                    exported: "default".to_string(),
                });
                for class in module.classes.iter_mut() {
                    if &class.name == class_name {
                        class.is_exported = true;
                        break;
                    }
                }
            } else {
                // For other expressions (closures, calls, etc.), create a synthetic "default" variable
                let id = ctx.define_local("default".to_string(), Type::Any);
                module.init.push(Stmt::Let {
                    id,
                    name: "default".to_string(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(lowered),
                });
                module.exported_objects.push("default".to_string());
                module.exports.push(Export::Named {
                    local: "default".to_string(),
                    exported: "default".to_string(),
                });
            }
        }
        _ => {
            // TsImportEquals, TsExportAssignment, TsNamespaceExport - TypeScript specific
        }
    }
    Ok(())
}

/// Lower a TypeScript namespace declaration into a synthetic class with static methods.
/// `export namespace Slug { export function create() { ... } }` becomes a class `Slug`
/// with a static method `create`. Exported namespace variables are lowered as module-level
/// locals (not static fields) and accessed via compile-time namespace resolution.
/// Private namespace members (non-exported) are lowered as module-level variables.
pub(crate) fn lower_namespace_as_class(
    ctx: &mut LoweringContext,
    module: &mut Module,
    ns_name: &str,
    body: &ast::TsNamespaceBody,
    is_exported: bool,
) -> Result<Class> {
    let class_id = match ctx.lookup_class(ns_name) {
        Some(id) => id,
        None => {
            let id = ctx.fresh_class();
            ctx.register_class(ns_name.to_string(), id);
            id
        }
    };

    let items = match body {
        ast::TsNamespaceBody::TsModuleBlock(block) => &block.body,
        ast::TsNamespaceBody::TsNamespaceDecl(_) => {
            // Nested namespace (namespace A.B { }) — not supported yet
            return Ok(Class {
                id: class_id,
                name: ns_name.to_string(),
                type_params: Vec::new(),
                extends: None,
                extends_name: None,
                native_extends: None,
                extends_expr: None,
                fields: Vec::new(),
                constructor: None,
                methods: Vec::new(),
                getters: Vec::new(),
                setters: Vec::new(),
                static_fields: Vec::new(),
                static_methods: Vec::new(),
                decorators: Vec::new(),
                is_exported,
                aliases: Vec::new(),
            });
        }
    };

    let mut static_methods = Vec::new();
    let mut static_method_names = Vec::new();

    // First pass: collect exported function names, pre-register all functions and variables
    // (so namespace members can reference each other regardless of declaration order)
    for item in items {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_some() {
                            let name = fn_decl.ident.sym.to_string();
                            static_method_names.push(name.clone());
                            // Pre-register exported functions so other namespace members can call them
                            if ctx.lookup_func(&name).is_none() {
                                let id = ctx.fresh_func();
                                ctx.register_func(name, id);
                            }
                        }
                    }
                    ast::Decl::Var(var_decl) => {
                        // Pre-register exported namespace variables as module-level locals
                        for decl in &var_decl.decls {
                            if let Ok(name) = get_binding_name(&decl.name) {
                                if ctx.lookup_local(&name).is_none() {
                                    let ty = extract_binding_type(&decl.name);
                                    ctx.define_local(name.clone(), ty);
                                    ctx.pre_registered_module_vars.insert(name);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Pre-register non-exported functions (hoisted like JS)
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => {
                if fn_decl.function.body.is_some() {
                    let name = fn_decl.ident.sym.to_string();
                    if ctx.lookup_func(&name).is_none() {
                        let id = ctx.fresh_func();
                        ctx.register_func(name, id);
                    }
                }
            }
            // Pre-register non-exported variables
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(var_decl))) => {
                for decl in &var_decl.decls {
                    if let ast::Pat::Ident(ident) = &decl.name {
                        let name = ident.id.sym.to_string();
                        if ctx.lookup_local(&name).is_none() {
                            let ty = ident
                                .type_ann
                                .as_ref()
                                .map(|ann| extract_ts_type(&ann.type_ann))
                                .unwrap_or(Type::Any);
                            ctx.define_local(name.clone(), ty);
                            ctx.pre_registered_module_vars.insert(name);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Register class and statics early so method bodies can reference them
    ctx.register_class_statics(ns_name.to_string(), Vec::new(), static_method_names.clone());

    // Set current namespace so internal function calls resolve as StaticMethodCall
    let prev_namespace = ctx.current_namespace.take();
    ctx.current_namespace = Some(ns_name.to_string());

    // Second pass: lower all items
    for item in items {
        match item {
            // Non-exported items → module-level variables/functions
            ast::ModuleItem::Stmt(stmt) => {
                lower_stmt(ctx, module, stmt)?;
            }
            // Exported items
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_none() {
                            continue; // Skip declare functions
                        }
                        let func = lower_fn_decl(ctx, fn_decl)?;
                        // Register return type for call-site inference
                        if !matches!(func.return_type, Type::Any) {
                            ctx.register_func_return_type(
                                func.name.clone(),
                                func.return_type.clone(),
                            );
                        }
                        if let Some((module, class)) =
                            native_instance_from_return_type(&func.return_type)
                        {
                            ctx.func_return_native_instances.push((
                                func.name.clone(),
                                module.to_string(),
                                class.to_string(),
                            ));
                        }
                        static_methods.push(func);
                    }
                    ast::Decl::Var(var_decl) => {
                        // Lower exported namespace variables as module-level locals
                        let mutable = var_decl.kind != ast::VarDeclKind::Const;
                        for decl in &var_decl.decls {
                            let name = get_binding_name(&decl.name)?;
                            let ty = extract_binding_type(&decl.name);
                            if let Some(init) = &decl.init {
                                let expr = lower_expr(ctx, init)?;
                                let id = if ctx.pre_registered_module_vars.remove(&name) {
                                    let id = ctx.lookup_local(&name).unwrap();
                                    if let Some((_, _, existing_ty)) =
                                        ctx.locals.iter_mut().rev().find(|(n, _, _)| n == &name)
                                    {
                                        *existing_ty = ty.clone();
                                    }
                                    id
                                } else {
                                    ctx.define_local(name.clone(), ty.clone())
                                };
                                module.init.push(Stmt::Let {
                                    id,
                                    name: name.clone(),
                                    ty,
                                    mutable,
                                    init: Some(expr),
                                });
                                // Track as namespace variable for Ns.member access resolution
                                ctx.namespace_vars
                                    .push((ns_name.to_string(), name.clone(), id));
                                // Export the variable for cross-module access
                                if is_exported {
                                    module.exported_objects.push(name.clone());
                                    module.exports.push(Export::Named {
                                        local: name.clone(),
                                        exported: name.clone(),
                                    });
                                }
                            }
                        }
                    }
                    ast::Decl::Class(class_decl) => {
                        let class = lower_class_decl(ctx, class_decl, is_exported)?;
                        push_class_dedup(module, class);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Restore previous namespace context
    ctx.current_namespace = prev_namespace;

    Ok(Class {
        id: class_id,
        name: ns_name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_fields: Vec::new(),
        static_methods,
        decorators: Vec::new(),
        is_exported,
        aliases: Vec::new(),
    })
}
