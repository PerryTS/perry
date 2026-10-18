//! TypeScript namespace → synthetic-class lowering — extracted from
//! `lower/module_decl.rs` (pure mechanical split, no logic changes).

use crate::types::Type;
use anyhow::Result;
use swc_common::Spanned;
use swc_ecma_ast as ast;

use super::*;

/// Lower a TypeScript namespace declaration into a synthetic class with static methods.
/// `export namespace Slug { export function create() { ... } }` becomes a class `Slug`
/// with a static method `create`. Exported namespace variables are lowered as module-level
/// locals (not static fields) and accessed via compile-time namespace resolution.
/// Private namespace members (non-exported) are lowered as module-level variables.
/// #5130: the simple-ident name of a (non-dotted) nested namespace, if it has a
/// body. `namespace A.B {}` (dotted form) and bodiless `declare` modules return
/// `None`.
fn nested_namespace_name(ts_module: &ast::TsModuleDecl) -> Option<String> {
    ts_module.body.as_ref()?;
    match &ts_module.id {
        ast::TsModuleName::Ident(ident) => Some(ident.sym.to_string()),
        ast::TsModuleName::Str(_) => None,
    }
}

/// Publish a namespace export at its declaration position. An initializer in
/// the field metadata prevents codegen's uninitialized-class-field pass from
/// creating every property before the namespace body executes. The inline set
/// below is authoritative, so the late initializer fallback skips this field.
fn publish_namespace_member(
    module: &mut Module,
    fields: &mut Vec<crate::ir::ClassField>,
    ns_name: &str,
    name: String,
    value: Expr,
    is_readonly: bool,
) {
    fields.push(crate::ir::ClassField {
        origin: crate::ClassFieldOrigin::Definition,
        name: name.clone(),
        key_expr: None,
        ty: Type::Any,
        init: Some(value.clone()),
        is_private: false,
        is_readonly,
        decorators: Vec::new(),
    });
    module.init.push(Stmt::Expr(Expr::StaticFieldSet {
        class_name: ns_name.to_string(),
        field_name: name,
        value: Box::new(value),
    }));
}

/// #5130: lower a namespace nested inside another (`namespace Outer { export
/// namespace Inner { ... } }`). The inner namespace becomes its own synthetic
/// class registered under the qualified name `Outer.Inner`, and the outer
/// namespace gains a static field `Inner` holding a `ClassRef` to it — so
/// `Outer.Inner` resolves to the inner namespace object and `Outer.Inner.member`
/// reads its statics (a runtime property/method access on a class-ref resolves
/// static fields/methods). Nesting recurses to any depth.
fn lower_nested_namespace(
    ctx: &mut LoweringContext,
    module: &mut Module,
    outer_ns_name: &str,
    ts_module: &ast::TsModuleDecl,
    ns_static_fields: &mut Vec<crate::ir::ClassField>,
) -> Result<()> {
    let Some(inner_name) = nested_namespace_name(ts_module) else {
        return Ok(());
    };
    let Some(body) = &ts_module.body else {
        return Ok(());
    };
    let qualified = format!("{outer_ns_name}.{inner_name}");
    let class = lower_namespace_as_class(ctx, module, &qualified, body, true)?;
    push_class_dedup(module, class);

    // Surface the inner namespace as a static field of the outer one, set to a
    // ClassRef to the inner class. Mirrors the const-member wiring above.
    publish_namespace_member(
        module,
        ns_static_fields,
        outer_ns_name,
        inner_name,
        Expr::ClassRef(qualified),
        true,
    );
    Ok(())
}

/// The statics a namespace body publishes: (static field names, static method
/// names). Exported functions with a body are static methods; exported
/// classes and enums and every nested namespace are static fields.
fn namespace_static_names(items: &[ast::ModuleItem]) -> (Vec<String>, Vec<String>) {
    let mut fields = Vec::new();
    let mut methods = Vec::new();
    for item in items {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) if fn_decl.function.body.is_some() => {
                        methods.push(fn_decl.ident.sym.to_string());
                    }
                    ast::Decl::Class(class_decl) => fields.push(class_decl.ident.sym.to_string()),
                    ast::Decl::TsEnum(enum_decl) if !enum_decl.declare => {
                        fields.push(enum_decl.id.sym.to_string());
                    }
                    // #5130: nested `export namespace Inner { ... }`.
                    ast::Decl::TsModule(ts_module) if !ts_module.declare => {
                        fields.extend(nested_namespace_name(ts_module));
                    }
                    _ => {}
                }
            }
            // #5130: nested non-exported `namespace Inner { ... }`.
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsModule(ts_module)))
                if !ts_module.declare =>
            {
                fields.extend(nested_namespace_name(ts_module));
            }
            _ => {}
        }
    }
    (fields, methods)
}

/// Register every module-level namespace's class and statics before any body
/// is lowered: a function above `namespace JsxNames { ... }` (TypeScript's
/// checker.ts) may legally read `JsxNames.X`, since the namespace binding is
/// module-scoped. `lower_namespace_as_class` reuses this registration.
pub(crate) fn pre_register_module_namespaces(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    for item in &ast_module.body {
        let ts_module = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsModule(m))) => m,
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::TsModule(m) => m,
                    _ => continue,
                }
            }
            _ => continue,
        };
        if ts_module.declare {
            continue;
        }
        let (Some(ns_name), Some(ast::TsNamespaceBody::TsModuleBlock(block))) =
            (nested_namespace_name(ts_module), &ts_module.body)
        else {
            continue;
        };
        if ctx.lookup_class(&ns_name).is_some() {
            continue;
        }
        let id = ctx.fresh_class();
        ctx.register_class(ns_name.clone(), id);
        let (fields, methods) = namespace_static_names(&block.body);
        ctx.register_class_statics(ns_name.clone(), fields, methods);
        // `Ns.x` for an exported variable reads its module-level local
        // (`lookup_namespace_var`); bind it now so an earlier reader resolves
        // to the same local the declaration later initializes.
        for item in &block.body {
            let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) = item else {
                continue;
            };
            let ast::Decl::Var(var_decl) = &export.decl else {
                continue;
            };
            for decl in &var_decl.decls {
                let ast::Pat::Ident(ident) = &decl.name else {
                    continue;
                };
                let name = ident.id.sym.to_string();
                let id = ctx.define_local(
                    format!("{ns_name}.{name}"),
                    extract_binding_type(&decl.name),
                );
                ctx.namespace_vars.push((ns_name.clone(), name, id));
            }
        }
    }
}

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
                heritage_lexically_shadowed: false,
                fields: Vec::new(),
                constructor: None,
                methods: Vec::new(),
                getters: Vec::new(),
                setters: Vec::new(),
                static_accessor_names: Vec::new(),
                static_accessor_fn_ids: Vec::new(),
                static_fields: Vec::new(),
                static_methods: Vec::new(),
                computed_members: Vec::new(),
                decorators: Vec::new(),
                is_exported,
                aliases: Vec::new(),
                is_nested: false,
                alloc_width_hint: 0,
                specialized_from: None,
            });
        }
    };

    let mut static_methods = Vec::new();
    // #5130: nested namespace names (`namespace G { export namespace Nested {} }`).
    // Each is surfaced as a static field on the outer namespace class holding a
    // `ClassRef` to the (recursively lowered) inner namespace class, so
    // `G.Nested` resolves to the inner namespace and `G.Nested.value` /
    // `G.Nested.f()` read its statics. Registered as static fields up-front so
    // `has_static_field` routes `G.Nested` to `StaticFieldGet`.
    let (ns_static_names, static_method_names) = namespace_static_names(items);
    // Namespace `export const` members surfaced as static fields so `Ns.member`
    // resolves CROSS-MODULE (the per-module `namespace_vars` local is invisible
    // to importers; only namespace FUNCTIONS — lowered as static methods —
    // crossed the boundary). The field's VALUE is copied from the const's local
    // by a `StaticFieldSet` appended to `module.init` right after the const's
    // own `Let`, so it is evaluated exactly once and in the right order. zod's
    // `util` namespace (`util.objectKeys`, …) is imported this way.
    let mut ns_static_fields: Vec<crate::ir::ClassField> = Vec::new();

    // Namespace classes have lexical names, even though their definitions live
    // in the module's class table. Qualify the registration keys so unrelated
    // namespaces can each declare (for example) Service, and pre-register them
    // before lowering sibling functions that reference a later declaration.
    let saved_class_renames = ctx.class_renames.clone();
    let mut saved_class_bindings = Vec::new();
    for item in items {
        let decl = match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => &export.decl,
            ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl,
            _ => continue,
        };
        if let ast::Decl::Class(class_decl) = decl {
            let name = class_decl.ident.sym.to_string();
            let qualified = format!("{ns_name}.{name}");
            if ctx.lookup_class(&qualified).is_none() {
                let id = ctx.fresh_class();
                ctx.register_class(qualified.clone(), id);
            }
            // Binding probes such as `typeof C` look up the source spelling
            // before lowering the value through `class_renames`. Give them a
            // scope-local alias to the same class-table entry as well.
            let index = ctx.classes_index[&qualified];
            saved_class_bindings
                .push((name.clone(), ctx.classes_index.insert(name.clone(), index)));
            ctx.class_renames
                .insert(name, (qualified, class_decl.span().lo.0));
        }
        // A nested namespace is read by its bare name throughout the outer
        // body, also above its declaration (TypeScript's `Parser` calls
        // `JSDocParser.parseJSDocComment(...)`). Alias it like a nested class
        // and register its statics now; its own lowering reuses both.
        if let ast::Decl::TsModule(ts_module) = decl {
            let (false, Some(name), Some(ast::TsNamespaceBody::TsModuleBlock(block))) = (
                ts_module.declare,
                nested_namespace_name(ts_module),
                &ts_module.body,
            ) else {
                continue;
            };
            let qualified = format!("{ns_name}.{name}");
            if ctx.lookup_class(&qualified).is_none() {
                let id = ctx.fresh_class();
                ctx.register_class(qualified.clone(), id);
                let (fields, methods) = namespace_static_names(&block.body);
                ctx.register_class_statics(qualified.clone(), fields, methods);
            }
            let index = ctx.classes_index[&qualified];
            saved_class_bindings
                .push((name.clone(), ctx.classes_index.insert(name.clone(), index)));
            ctx.class_renames
                .insert(name, (qualified, ts_module.span().lo.0));
        }
    }

    // A namespace body is its own scope: its variables shadow same-named
    // outer bindings and go out of scope at its end (TypeScript's
    // `namespace Parser { var SourceFileConstructor }` beside parser.ts's
    // module-level `let SourceFileConstructor`). Locals from here on are
    // renamed `Ns.name` at the end, keeping their ids and types.
    let locals_mark = ctx.locals.len();
    // Functions likewise: parser.ts's module-level `createSourceFile` and
    // `namespace Parser { function createSourceFile }` are two functions.
    let functions_mark = ctx.functions.len();

    // First pass: collect exported function names, pre-register all functions and variables
    // (so namespace members can reference each other regardless of declaration order)
    for item in items {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) if fn_decl.function.body.is_some() => {
                        let name = fn_decl.ident.sym.to_string();
                        // Pre-register exported functions so other namespace members can call them
                        let id = ctx.fresh_func();
                        ctx.register_func(name, id);
                    }
                    ast::Decl::Var(var_decl) => {
                        // Pre-register exported namespace variables as module-level
                        // locals scoped to this body (see `locals_mark`).
                        for decl in &var_decl.decls {
                            if let Ok(name) = get_binding_name(&decl.name) {
                                let ty = extract_binding_type(&decl.name);
                                match ctx.lookup_namespace_var(ns_name, &name) {
                                    Some(id) => ctx.locals.push((name.clone(), id, ty)),
                                    None => {
                                        ctx.define_local(name.clone(), ty);
                                    }
                                }
                                ctx.pre_registered_module_vars.insert(name.clone());
                                if var_decl.kind == ast::VarDeclKind::Var {
                                    ctx.pre_registered_module_var_decls.insert(name);
                                }
                            }
                        }
                    }
                    // Enums are block-scoped to the namespace body, so a
                    // member function above the declaration may use them.
                    ast::Decl::TsEnum(enum_decl) => {
                        crate::lower::pre_scan::pre_register_enum(enum_decl, ctx);
                    }
                    _ => {}
                }
            }
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsEnum(enum_decl))) => {
                crate::lower::pre_scan::pre_register_enum(enum_decl, ctx);
            }
            // Pre-register non-exported functions (hoisted like JS)
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl)))
                if fn_decl.function.body.is_some() =>
            {
                let name = fn_decl.ident.sym.to_string();
                let id = ctx.fresh_func();
                ctx.register_func(name, id);
            }
            // Pre-register non-exported variables
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(var_decl))) => {
                for decl in &var_decl.decls {
                    if crate::lower::ambient::declarator_binds_nothing(var_decl, decl) {
                        continue;
                    }
                    if let ast::Pat::Ident(ident) = &decl.name {
                        let name = ident.id.sym.to_string();
                        let ty = ident
                            .type_ann
                            .as_ref()
                            .map(|ann| extract_ts_type(&ann.type_ann))
                            .unwrap_or(Type::Any);
                        ctx.define_local(name.clone(), ty);
                        ctx.pre_registered_module_vars.insert(name.clone());
                        if var_decl.kind == ast::VarDeclKind::Var {
                            ctx.pre_registered_module_var_decls.insert(name);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Register class and statics early so method bodies can reference them.
    // Nested namespace names are registered as static fields so `Outer.Inner`
    // resolves via `has_static_field` → `StaticFieldGet` (#5130).
    ctx.register_class_statics(
        ns_name.to_string(),
        ns_static_names,
        static_method_names.clone(),
    );

    // Set current namespace so internal function calls resolve as StaticMethodCall
    let prev_namespace = ctx.current_namespace.take();
    ctx.current_namespace = Some(ns_name.to_string());

    // Second pass: lower all items
    for item in items {
        match item {
            // #5130: nested non-exported `namespace Inner { ... }` — surface as a
            // static field of the outer namespace (same as the exported form)
            // rather than letting `lower_stmt` register it as a top-level
            // namespace with an unqualified name.
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsModule(ts_module)))
                if !ts_module.declare && nested_namespace_name(ts_module).is_some() =>
            {
                lower_nested_namespace(ctx, module, ns_name, ts_module, &mut ns_static_fields)?;
            }
            // A non-exported function is a module-level function named
            // `Ns.f`, so it never shares a symbol with a same-named
            // module-level function.
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl)))
                if fn_decl.function.body.is_some() =>
            {
                crate::lower::stmt::lower_module_fn_decl(ctx, module, fn_decl, Some(ns_name))?;
            }
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
                            ctx.push_func_return_native_instance((
                                func.name.clone(),
                                module.to_string(),
                                class.to_string(),
                            ));
                        }
                        // Keep direct static-method dispatch, and also publish
                        // its function value as an enumerable namespace member
                        // alongside classes and variables in source order.
                        let name = func.name.clone();
                        static_methods.push(func);
                        publish_namespace_member(
                            module,
                            &mut ns_static_fields,
                            ns_name,
                            name.clone(),
                            Expr::IndexGet {
                                object: Box::new(Expr::ClassRef(ns_name.to_string())),
                                index: Box::new(Expr::String(name)),
                            },
                            false,
                        );
                    }
                    ast::Decl::Var(var_decl) => {
                        // Lower exported namespace variables as module-level locals
                        let mutable = var_decl.kind != ast::VarDeclKind::Const;
                        let is_var = var_decl.kind == ast::VarDeclKind::Var;
                        for decl in &var_decl.decls {
                            if is_destructuring_pattern(&decl.name) {
                                let mut names = Vec::new();
                                collect_binding_names(&decl.name, &mut names);
                                if decl.init.is_some() {
                                    let stmts = lower_var_decl_with_destructuring(
                                        ctx, decl, mutable, is_var,
                                    )?;
                                    module.init.extend(stmts);
                                    for name in names {
                                        if let Some(id) = ctx.lookup_local(&name) {
                                            ctx.namespace_vars.push((
                                                ns_name.to_string(),
                                                name.clone(),
                                                id,
                                            ));
                                        }
                                        if is_exported {
                                            module.exported_objects.push(name.clone());
                                            module.exports.push(Export::Named {
                                                local: name.clone(),
                                                exported: name,
                                            });
                                        }
                                    }
                                    continue;
                                }
                            }

                            let name = get_binding_name(&decl.name)?;
                            let ty = extract_binding_type(&decl.name);
                            if let Some(init) = &decl.init {
                                let expr = lower_expr(ctx, init)?;
                                let id = if ctx.pre_registered_module_vars.remove(&name) {
                                    ctx.pre_registered_module_var_decls.remove(&name);
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
                                ctx.record_local_source_span(id, decl.name.span());
                                module.init.push(Stmt::Let {
                                    id,
                                    name: name.clone(),
                                    ty,
                                    mutable,
                                    init: Some(expr),
                                });
                                // Track as namespace variable for `Ns.member`
                                // access AND intra-namespace bare references.
                                ctx.namespace_vars
                                    .push((ns_name.to_string(), name.clone(), id));
                                // Surface as a static field of the namespace class
                                // and copy the const's value into it (after the Let
                                // above), so `Ns.member` resolves cross-module via
                                // the static-field global without re-evaluation.
                                if is_exported {
                                    publish_namespace_member(
                                        module,
                                        &mut ns_static_fields,
                                        ns_name,
                                        name.clone(),
                                        Expr::LocalGet(id),
                                        !mutable,
                                    );
                                }
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
                        let class_name = class.name.clone();
                        // A declaration evaluates heritage and computed keys
                        // before static fields/blocks and legacy decorators,
                        // just like the module-level class declaration path.
                        if let Some(extends_expr) = &class.extends_expr {
                            module
                                .init
                                .push(Stmt::Expr(Expr::RegisterClassParentDynamic {
                                    class_name: class_name.clone(),
                                    parent_expr: extends_expr.clone(),
                                }));
                        }
                        let (computed_name_evaluations, _) =
                            crate::lower_decl::prepare_ordered_class_computed_names(
                                &class_decl.class.body,
                                &class,
                                &class_name,
                            );
                        module
                            .init
                            .extend(computed_name_evaluations.into_iter().map(Stmt::Expr));
                        module.init.extend(
                            crate::lower_decl::build_interleaved_static_init_stmts_after_computed_names(
                                &class_decl.class.body,
                                &class_name,
                                &class.fields,
                                &class.static_fields,
                                &class.static_methods,
                            ),
                        );
                        append_legacy_decorator_init_for_class(ctx, &mut module.init, &class);
                        push_class_dedup(module, class);

                        // `export` here publishes on the namespace regardless
                        // of whether the namespace itself is a module export.
                        // Append the field at its source position to preserve
                        // enumeration order alongside exported variables.
                        publish_namespace_member(
                            module,
                            &mut ns_static_fields,
                            ns_name,
                            class_decl.ident.sym.to_string(),
                            Expr::ClassRef(class_name),
                            false,
                        );
                    }
                    ast::Decl::TsEnum(enum_decl) => {
                        let en = lower_enum_decl(ctx, enum_decl, false)?;
                        let name = en.name.clone();
                        let own = en.members.clone();
                        module.enums.push(en);
                        if enum_decl.declare {
                            continue;
                        }
                        if let Some((id, bind)) =
                            crate::lower_decl::enum_value_binding(ctx, &name, true, &own)
                        {
                            module.init.push(bind);
                            ctx.namespace_vars
                                .push((ns_name.to_string(), name.clone(), id));
                            publish_namespace_member(
                                module,
                                &mut ns_static_fields,
                                ns_name,
                                name,
                                Expr::LocalGet(id),
                                false,
                            );
                        }
                    }
                    // #5130: nested `export namespace Inner { ... }`.
                    ast::Decl::TsModule(ts_module) => {
                        lower_nested_namespace(
                            ctx,
                            module,
                            ns_name,
                            ts_module,
                            &mut ns_static_fields,
                        )?;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Restore previous namespace context
    ctx.current_namespace = prev_namespace;
    ctx.truncate_functions(functions_mark);
    let scoped: Vec<_> = ctx.locals.drain_from(locals_mark);
    for (name, id, ty) in scoped {
        if ctx.sloppy_implicit_global_ids.contains(&id) {
            ctx.locals.push((name, id, ty));
        } else {
            ctx.locals.push((format!("{ns_name}.{name}"), id, ty));
        }
    }
    ctx.class_renames = saved_class_renames;
    for (name, previous) in saved_class_bindings.into_iter().rev() {
        if let Some(index) = previous {
            ctx.classes_index.insert(name, index);
        } else {
            ctx.classes_index.remove(&name);
        }
    }

    Ok(Class {
        id: class_id,
        name: ns_name.to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: Vec::new(),
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        static_fields: ns_static_fields,
        static_methods,
        computed_members: Vec::new(),
        decorators: Vec::new(),
        is_exported,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    })
}
