//! TypeScript namespaces lower to what tsc emits for them:
//!
//! ```js
//! var N;
//! (function (N) { N.x = 1; function f() {} N.f = f; })(N || (N = {}));
//! ```
//!
//! The namespace value is a plain object, or the class, function or enum it
//! merges with. Every export is an ordinary data property, stored at its source
//! position. The body stays module-level code: its functions are module
//! functions and its bindings module-level locals scoped to the body. As in
//! tsc, a reference to an exported variable, or to a member that another block
//! of the same namespace exports, reads and writes that property.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};

use crate::types::Type;
use anyhow::Result;
use swc_common::{Globals, Mark, Spanned, SyntaxContext, DUMMY_SP, GLOBALS};
use swc_ecma_ast as ast;
use swc_ecma_visit::{VisitMut, VisitMutWith};

use super::*;

/// The binding name of a namespace with a body: `namespace N {}`, and `A` for
/// the dotted `namespace A.B {}`. Bodiless and string-named (`declare module
/// "x"`) modules bind nothing.
fn namespace_name(ts_module: &ast::TsModuleDecl) -> Option<String> {
    ts_module.body.as_ref()?;
    match &ts_module.id {
        ast::TsModuleName::Ident(ident) => Some(ident.sym.to_string()),
        ast::TsModuleName::Str(_) => None,
    }
}

/// The items of a namespace body. `namespace A.B { … }` is
/// `namespace A { export namespace B { … } }`.
fn namespace_items(body: &ast::TsNamespaceBody) -> Cow<'_, [ast::ModuleItem]> {
    match body {
        ast::TsNamespaceBody::TsModuleBlock(block) => Cow::Borrowed(&block.body),
        ast::TsNamespaceBody::TsNamespaceDecl(decl) => {
            Cow::Owned(vec![ast::ModuleItem::ModuleDecl(
                ast::ModuleDecl::ExportDecl(ast::ExportDecl {
                    span: decl.span,
                    decl: ast::Decl::TsModule(Box::new(ast::TsModuleDecl {
                        span: decl.span,
                        declare: decl.declare,
                        global: decl.global,
                        namespace: true,
                        id: ast::TsModuleName::Ident(decl.id.clone()),
                        body: Some((*decl.body).clone()),
                    })),
                }),
            )])
        }
    }
}

/// The names a namespace block exports, i.e. the properties it stores on the
/// namespace object.
fn exported_names(items: &[ast::ModuleItem]) -> Vec<String> {
    let mut names = Vec::new();
    for item in items {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) = item else {
            continue;
        };
        match &export.decl {
            ast::Decl::Fn(fn_decl) if fn_decl.function.body.is_some() => {
                names.push(fn_decl.ident.sym.to_string());
            }
            ast::Decl::Class(class_decl) if !class_decl.declare => {
                names.push(class_decl.ident.sym.to_string());
            }
            ast::Decl::TsEnum(enum_decl) if !enum_decl.declare => {
                names.push(enum_decl.id.sym.to_string());
            }
            ast::Decl::TsModule(ts_module) if !ts_module.declare => {
                names.extend(namespace_name(ts_module));
            }
            ast::Decl::Var(var_decl) if !var_decl.declare => {
                for decl in &var_decl.decls {
                    collect_binding_names(&decl.name, &mut names);
                }
            }
            _ => {}
        }
    }
    names
}

/// The namespace declarations among a module's or namespace's items, with
/// whether each is exported.
fn namespace_decls(
    items: &[ast::ModuleItem],
) -> impl Iterator<Item = (&ast::TsModuleDecl, bool)> + '_ {
    items.iter().filter_map(|item| match item {
        ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsModule(m))) => Some((&**m, false)),
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => match &export.decl {
            ast::Decl::TsModule(m) => Some((&**m, true)),
            _ => None,
        },
        _ => None,
    })
}

/// Bind every module-level namespace before any body is lowered: a function
/// above `namespace JsxNames { ... }` (TypeScript's checker.ts) may read
/// `JsxNames.X`, since the binding is module-scoped (tsc's `var JsxNames`).
/// A namespace merging into a module-level class, function or enum binds
/// nothing of its own. Also records every block's exports, which the other
/// blocks of the same namespace reach as properties.
pub(crate) fn pre_register_module_namespaces(ast_module: &ast::Module, ctx: &mut LoweringContext) {
    let mut merged = HashSet::new();
    for item in &ast_module.body {
        let decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl,
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => &export.decl,
            _ => continue,
        };
        match decl {
            ast::Decl::Class(class_decl) => merged.insert(class_decl.ident.sym.to_string()),
            ast::Decl::Fn(fn_decl) => merged.insert(fn_decl.ident.sym.to_string()),
            _ => false,
        };
    }
    for (ts_module, _) in namespace_decls(&ast_module.body) {
        let (false, Some(name), Some(body)) = (
            ts_module.declare,
            namespace_name(ts_module),
            &ts_module.body,
        ) else {
            continue;
        };
        ctx.namespace_exports
            .entry(name.clone())
            .or_default()
            .extend(exported_names(&namespace_items(body)));
        if merged.contains(&name) || ctx.lookup_local(&name).is_some() {
            continue;
        }
        let id = ctx.define_local(name.clone(), Type::Any);
        ctx.namespace_locals.insert(id);
        ctx.pre_registered_module_vars.insert(name.clone());
        ctx.pre_registered_module_var_decls.insert(name);
    }
}

/// Lower a module-level `namespace N { … }` block.
pub(crate) fn lower_namespace(
    ctx: &mut LoweringContext,
    module: &mut Module,
    ts_module: &ast::TsModuleDecl,
    is_exported: bool,
) -> Result<()> {
    lower_namespace_block(ctx, module, ts_module, is_exported, None)
}

/// The block a nested namespace is declared in: its object's local and its
/// qualified name.
#[derive(Clone, Copy)]
struct Outer<'a> {
    object: LocalId,
    qualified: &'a str,
}

/// The namespace object a block extends, as tsc's `N || (N = {})` (or
/// `Inner = Outer.Inner || (Outer.Inner = {})` for an exported nested one)
/// finds or creates it.
fn namespace_object(
    ctx: &mut LoweringContext,
    module: &mut Module,
    name: &str,
    is_exported: bool,
    outer: Option<Outer<'_>>,
) -> Result<Expr> {
    match ctx.lookup_local(name) {
        // The namespace's own binding: still pending for its first block,
        // already holding the object for a re-opened one.
        Some(id) if ctx.namespace_locals.contains(&id) => {
            if ctx.pre_registered_module_vars.remove(name) {
                ctx.pre_registered_module_var_decls.remove(name);
                let init = match outer {
                    Some(outer) if is_exported => {
                        let published = || Expr::PropertyGet {
                            byte_end: 0,
                            object: Box::new(Expr::LocalGet(outer.object)),
                            property: name.to_string(),
                            byte_offset: 0,
                        };
                        Expr::Logical {
                            op: LogicalOp::Or,
                            left: Box::new(published()),
                            right: Box::new(Expr::PropertySet {
                                object: Box::new(Expr::LocalGet(outer.object)),
                                property: name.to_string(),
                                value: Box::new(Expr::Object(Vec::new())),
                            }),
                        }
                    }
                    _ => Expr::Object(Vec::new()),
                };
                module.init.push(Stmt::Let {
                    id,
                    name: name.to_string(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(init),
                });
                if outer.is_none() && is_exported {
                    module.exported_objects.push(name.to_string());
                    module.exports.push(Export::Named {
                        local: name.to_string(),
                        exported: name.to_string(),
                    });
                }
            }
            Ok(Expr::LocalGet(id))
        }
        // Declaration merging: the namespace extends the class, function or
        // enum of the same name.
        _ => {
            let ident = ast::Ident::new_no_ctxt(name.into(), DUMMY_SP);
            lower_expr(ctx, &ast::Expr::Ident(ident))
        }
    }
}

fn lower_namespace_block(
    ctx: &mut LoweringContext,
    module: &mut Module,
    ts_module: &ast::TsModuleDecl,
    is_exported: bool,
    outer: Option<Outer<'_>>,
) -> Result<()> {
    let (false, Some(name), Some(body)) = (
        ts_module.declare,
        namespace_name(ts_module),
        &ts_module.body,
    ) else {
        return Ok(());
    };
    let qualified = match outer {
        Some(outer) => format!("{}.{name}", outer.qualified),
        None => name.clone(),
    };
    let object = namespace_object(ctx, module, &name, is_exported, outer)?;

    // A namespace body is its own scope: its bindings shadow same-named outer
    // ones and go out of scope at its end (TypeScript's `namespace Parser {
    // var SourceFileConstructor }` beside parser.ts's module-level `let
    // SourceFileConstructor`). Locals from here on are renamed `Ns.name` at
    // the end, keeping their ids and types.
    let locals_mark = ctx.locals.len();
    // Functions likewise: parser.ts's module-level `createSourceFile` and
    // `namespace Parser { function createSourceFile }` are two functions.
    let functions_mark = ctx.functions.len();

    // The body names its object through a binding no source name can shadow
    // (tsc's IIFE parameter).
    let alias = format!("{qualified}#{}", ts_module.span.lo.0);
    let object_id = match object {
        Expr::LocalGet(id) => {
            ctx.locals.push((alias.clone(), id, Type::Any));
            id
        }
        merged => {
            let id = ctx.define_local(alias.clone(), Type::Any);
            module.init.push(Stmt::Let {
                id,
                name: alias.clone(),
                ty: Type::Any,
                mutable: false,
                init: Some(merged),
            });
            id
        }
    };

    let items = namespace_items(body);
    let own_names = exported_names(&items);
    let other_blocks: BTreeSet<String> = ctx
        .namespace_exports
        .get(&qualified)
        .map(|names| {
            names
                .iter()
                .filter(|n| !own_names.contains(n))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let exports_vars = items.iter().any(|item| {
        matches!(item, ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export))
            if matches!(&export.decl, ast::Decl::Var(var_decl) if !var_decl.declare))
    });
    let items: Cow<'_, [ast::ModuleItem]> = if exports_vars || !other_blocks.is_empty() {
        Cow::Owned(rewrite_member_references(
            items.into_owned(),
            &alias,
            &other_blocks,
        ))
    } else {
        items
    };

    // Classes declared in the body keep lexical names, though their
    // definitions live in the module's class table. Qualify the registration
    // keys so unrelated namespaces can each declare (for example) Service, and
    // pre-register them before lowering sibling functions that reference a
    // later declaration.
    let saved_class_renames = ctx.class_renames.clone();
    let mut saved_class_bindings = Vec::new();
    let mut body_classes = HashSet::new();
    for item in items.iter() {
        let decl = match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => &export.decl,
            ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl,
            _ => continue,
        };
        if let ast::Decl::Class(class_decl) = decl {
            let name = class_decl.ident.sym.to_string();
            body_classes.insert(name.clone());
            let qualified = format!("{qualified}.{name}");
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
    }

    // Bind the body's functions, variables, enums and nested namespaces up
    // front: members reference each other regardless of declaration order
    // (TypeScript's `Parser` calls `JSDocParser.parseJSDocComment(...)` above
    // the nested namespace).
    for item in items.iter() {
        let decl = match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => &export.decl,
            ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl,
            _ => continue,
        };
        match decl {
            ast::Decl::Fn(fn_decl) if fn_decl.function.body.is_some() => {
                let id = ctx.fresh_func();
                ctx.register_func(fn_decl.ident.sym.to_string(), id);
            }
            ast::Decl::TsEnum(enum_decl) => {
                crate::lower::pre_scan::pre_register_enum(enum_decl, ctx);
            }
            // Exported variables were rewritten to property stores above, so
            // every declaration left binds a body-scoped local.
            ast::Decl::Var(var_decl) => {
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
            ast::Decl::TsModule(ts_module) if !ts_module.declare => {
                let (Some(name), Some(body)) = (namespace_name(ts_module), &ts_module.body) else {
                    continue;
                };
                ctx.namespace_exports
                    .entry(format!("{qualified}.{name}"))
                    .or_default()
                    .extend(exported_names(&namespace_items(body)));
                // A re-opened nested namespace shares the binding; one that
                // merges into a class, function or enum of the body (declared
                // above it, as TypeScript requires) binds nothing.
                let bound_here = ctx
                    .lookup_local_index(&name)
                    .is_some_and(|index| index >= locals_mark)
                    || ctx
                        .functions_index
                        .get(&name)
                        .is_some_and(|&index| index >= functions_mark)
                    || body_classes.contains(&name);
                if !bound_here {
                    let id = ctx.define_local(name.clone(), Type::Any);
                    ctx.namespace_locals.insert(id);
                    ctx.pre_registered_module_vars.insert(name.clone());
                    ctx.pre_registered_module_var_decls.insert(name);
                }
            }
            _ => {}
        }
    }

    let outer_here = Some(Outer {
        object: object_id,
        qualified: &qualified,
    });
    let publish = |module: &mut Module, name: String, value: Expr| {
        module.init.push(Stmt::Expr(Expr::PropertySet {
            object: Box::new(Expr::LocalGet(object_id)),
            property: name,
            value: Box::new(value),
        }));
    };
    for item in items.iter() {
        match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::TsModule(ts_module))) => {
                lower_namespace_block(ctx, module, ts_module, false, outer_here)?;
            }
            // A body function is a module-level function named `Ns.f`, so it
            // never shares a symbol with a same-named module-level function.
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => {
                crate::lower::stmt::lower_module_fn_decl(ctx, module, fn_decl, Some(&qualified))?;
            }
            ast::ModuleItem::Stmt(stmt) => lower_stmt(ctx, module, stmt)?,
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                match &export.decl {
                    ast::Decl::Fn(fn_decl) => {
                        if fn_decl.function.body.is_none() {
                            continue;
                        }
                        crate::lower::stmt::lower_module_fn_decl(
                            ctx,
                            module,
                            fn_decl,
                            Some(&qualified),
                        )?;
                        let name = fn_decl.ident.sym.to_string();
                        let id = ctx
                            .lookup_func(&name)
                            .expect("namespace function is pre-registered");
                        publish(module, name, Expr::FuncRef(id));
                    }
                    ast::Decl::Class(class_decl) if !class_decl.declare => {
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
                        publish(
                            module,
                            class_decl.ident.sym.to_string(),
                            Expr::ClassRef(class_name),
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
                        if let Some((id, bind)) = crate::lower_decl::enum_value_binding(
                            ctx, &name, true, &own, enum_decl,
                        )? {
                            module.init.push(bind);
                            publish(module, name, Expr::LocalGet(id));
                        }
                    }
                    ast::Decl::TsModule(ts_module) => {
                        lower_namespace_block(ctx, module, ts_module, true, outer_here)?;
                    }
                    // Exported variables are statements now; ambient
                    // declarations, interfaces and type aliases have no value.
                    _ => {}
                }
            }
            _ => {}
        }
    }

    ctx.truncate_functions(functions_mark);
    let scoped: Vec<_> = ctx.locals.drain_from(locals_mark);
    for (name, id, ty) in scoped {
        if ctx.sloppy_implicit_global_ids.contains(&id) {
            ctx.locals.push((name, id, ty));
        } else {
            ctx.locals.push((format!("{qualified}.{name}"), id, ty));
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
    Ok(())
}

/// tsc's rewrite of a namespace body: an exported variable is a property of
/// the namespace object, so its declaration becomes a store (`export const x
/// = 1` is `N.x = 1`; one without an initializer stores nothing) and every
/// reference to it reads or writes `N.x`. A name another block of the same
/// namespace exports is likewise `N.name` wherever the body does not bind it
/// itself. `alias` is the body's binding for its namespace object.
fn rewrite_member_references(
    items: Vec<ast::ModuleItem>,
    alias: &str,
    other_blocks: &BTreeSet<String>,
) -> Vec<ast::ModuleItem> {
    GLOBALS.set(&Globals::new(), || {
        let unresolved = Mark::new();
        let mut body = ast::Module {
            span: DUMMY_SP,
            body: items,
            shebang: None,
        };
        body.visit_mut_with(&mut swc_ecma_transforms_base::resolver(
            unresolved,
            Mark::new(),
            true,
        ));
        let mut exported = HashSet::new();
        for item in &body.body {
            if let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) = item {
                if let ast::Decl::Var(var_decl) = &export.decl {
                    if !var_decl.declare {
                        for decl in &var_decl.decls {
                            binding_ids(&decl.name, &mut exported);
                        }
                    }
                }
            }
        }
        let mut rewrite = MemberReferences {
            alias,
            exported: &exported,
            other_blocks,
            unresolved: SyntaxContext::empty().apply_mark(unresolved),
        };
        let mut items = Vec::with_capacity(body.body.len());
        for mut item in body.body {
            if let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) = &mut item {
                if let ast::Decl::Var(var_decl) = &mut export.decl {
                    if !var_decl.declare {
                        for decl in std::mem::take(&mut var_decl.decls) {
                            let Some(mut init) = decl.init else {
                                continue;
                            };
                            let mut target = decl.name;
                            target.visit_mut_with(&mut rewrite);
                            init.visit_mut_with(&mut rewrite);
                            let left = ast::AssignTarget::try_from(target)
                                .expect("an exported binding pattern is an assignment target");
                            items.push(ast::ModuleItem::Stmt(ast::Stmt::Expr(ast::ExprStmt {
                                span: decl.span,
                                expr: Box::new(ast::Expr::Assign(ast::AssignExpr {
                                    span: decl.span,
                                    op: ast::AssignOp::Assign,
                                    left,
                                    right: init,
                                })),
                            })));
                        }
                        continue;
                    }
                }
            }
            item.visit_mut_with(&mut rewrite);
            items.push(item);
        }
        // HIR uses lexical names; resolver contexts must not escape GLOBALS.
        items.visit_mut_with(&mut ClearContexts);
        items
    })
}

/// The identities a binding pattern declares.
fn binding_ids(pat: &ast::Pat, out: &mut HashSet<ast::Id>) {
    match pat {
        ast::Pat::Ident(ident) => {
            out.insert(ident.id.to_id());
        }
        ast::Pat::Array(array) => {
            for elem in array.elems.iter().flatten() {
                binding_ids(elem, out);
            }
        }
        ast::Pat::Object(object) => {
            for prop in &object.props {
                match prop {
                    ast::ObjectPatProp::KeyValue(kv) => binding_ids(&kv.value, out),
                    ast::ObjectPatProp::Assign(assign) => {
                        out.insert(assign.key.id.to_id());
                    }
                    ast::ObjectPatProp::Rest(rest) => binding_ids(&rest.arg, out),
                }
            }
        }
        ast::Pat::Rest(rest) => binding_ids(&rest.arg, out),
        ast::Pat::Assign(assign) => binding_ids(&assign.left, out),
        ast::Pat::Expr(_) | ast::Pat::Invalid(_) => {}
    }
}

struct MemberReferences<'a> {
    alias: &'a str,
    exported: &'a HashSet<ast::Id>,
    other_blocks: &'a BTreeSet<String>,
    unresolved: SyntaxContext,
}

impl MemberReferences<'_> {
    fn is_member(&self, ident: &ast::Ident) -> bool {
        self.exported.contains(&ident.to_id())
            || (ident.ctxt == self.unresolved && self.other_blocks.contains(ident.sym.as_ref()))
    }

    fn member(&self, ident: &ast::Ident) -> ast::MemberExpr {
        ast::MemberExpr {
            span: ident.span,
            obj: Box::new(ast::Expr::Ident(ast::Ident::new_no_ctxt(
                self.alias.into(),
                ident.span,
            ))),
            prop: ast::MemberProp::Ident(ast::IdentName::new(ident.sym.clone(), ident.span)),
        }
    }

    fn member_pat(&self, ident: &ast::Ident) -> ast::Pat {
        ast::Pat::Expr(Box::new(ast::Expr::Member(self.member(ident))))
    }
}

impl VisitMut for MemberReferences<'_> {
    fn visit_mut_expr(&mut self, expr: &mut ast::Expr) {
        if let ast::Expr::Ident(ident) = expr {
            if self.is_member(ident) {
                *expr = ast::Expr::Member(self.member(ident));
            }
            return;
        }
        expr.visit_mut_children_with(self);
    }

    fn visit_mut_prop(&mut self, prop: &mut ast::Prop) {
        if let ast::Prop::Shorthand(ident) = prop {
            if self.is_member(ident) {
                *prop = ast::Prop::KeyValue(ast::KeyValueProp {
                    key: ast::PropName::Ident(ast::IdentName::new(ident.sym.clone(), ident.span)),
                    value: Box::new(ast::Expr::Member(self.member(ident))),
                });
            }
            return;
        }
        prop.visit_mut_children_with(self);
    }

    fn visit_mut_pat(&mut self, pat: &mut ast::Pat) {
        if let ast::Pat::Ident(binding) = pat {
            if self.is_member(&binding.id) {
                *pat = self.member_pat(&binding.id);
            }
            return;
        }
        pat.visit_mut_children_with(self);
    }

    fn visit_mut_object_pat_prop(&mut self, prop: &mut ast::ObjectPatProp) {
        if let ast::ObjectPatProp::Assign(assign) = prop {
            if self.is_member(&assign.key.id) {
                let target = self.member_pat(&assign.key.id);
                let value = match assign.value.take() {
                    Some(default) => ast::Pat::Assign(ast::AssignPat {
                        span: assign.span,
                        left: Box::new(target),
                        right: default,
                    }),
                    None => target,
                };
                *prop = ast::ObjectPatProp::KeyValue(ast::KeyValuePatProp {
                    key: ast::PropName::Ident(ast::IdentName::new(
                        assign.key.id.sym.clone(),
                        assign.key.id.span,
                    )),
                    value: Box::new(value),
                });
            }
        }
        prop.visit_mut_children_with(self);
    }

    fn visit_mut_simple_assign_target(&mut self, target: &mut ast::SimpleAssignTarget) {
        if let ast::SimpleAssignTarget::Ident(binding) = target {
            if self.is_member(&binding.id) {
                *target = ast::SimpleAssignTarget::Member(self.member(&binding.id));
            }
            return;
        }
        target.visit_mut_children_with(self);
    }
}

struct ClearContexts;
impl VisitMut for ClearContexts {
    fn visit_mut_syntax_context(&mut self, ctxt: &mut SyntaxContext) {
        *ctxt = SyntaxContext::empty();
    }
    fn visit_mut_span(&mut self, _: &mut swc_common::Span) {}
}
