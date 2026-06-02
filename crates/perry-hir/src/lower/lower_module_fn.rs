//! Module-level lowering entry points: `lower_module` and the
//! `lower_module_with_class_id*` family.
//!
//! Extracted from `lower/mod.rs`. These are the public entry points
//! that drive the entire AST → HIR conversion for a single module.
//! All seven `pub fn` wrappers remain public; downstream callers
//! reach them via `crate::lower::lower_module*` (or the `lib.rs`
//! re-exports — `pub use lower::{lower_module, ...}`).

use anyhow::Result;
use perry_types::Type;
use std::collections::HashSet;
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

fn stmt_is_string_directive(stmt: &ast::Stmt) -> Option<&str> {
    let ast::Stmt::Expr(expr_stmt) = stmt else {
        return None;
    };
    let mut expr = expr_stmt.expr.as_ref();
    while let ast::Expr::Paren(paren) = expr {
        expr = paren.expr.as_ref();
    }
    let ast::Expr::Lit(ast::Lit::Str(s)) = expr else {
        return None;
    };
    s.value.as_str()
}

fn module_has_strict_mode(ast_module: &ast::Module) -> bool {
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::ModuleDecl(_) => return true,
            ast::ModuleItem::Stmt(stmt) => {
                let Some(directive) = stmt_is_string_directive(stmt) else {
                    break;
                };
                if directive == "use strict" {
                    return true;
                }
            }
        }
    }
    false
}

fn collect_assigned_function_binding_candidates(ast_module: &ast::Module) -> HashSet<String> {
    fn collect_from_stmt(stmt: &ast::Stmt, out: &mut HashSet<String>) {
        match stmt {
            ast::Stmt::Block(block) => {
                for stmt in &block.stmts {
                    collect_from_stmt(stmt, out);
                }
            }
            ast::Stmt::Expr(expr_stmt) => collect_from_expr(&expr_stmt.expr, out),
            ast::Stmt::If(if_stmt) => {
                collect_from_expr(&if_stmt.test, out);
                collect_from_stmt(&if_stmt.cons, out);
                if let Some(alt) = &if_stmt.alt {
                    collect_from_stmt(alt, out);
                }
            }
            ast::Stmt::While(while_stmt) => {
                collect_from_expr(&while_stmt.test, out);
                collect_from_stmt(&while_stmt.body, out);
            }
            ast::Stmt::DoWhile(do_while) => {
                collect_from_stmt(&do_while.body, out);
                collect_from_expr(&do_while.test, out);
            }
            ast::Stmt::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        ast::VarDeclOrExpr::Expr(expr) => collect_from_expr(expr, out),
                        ast::VarDeclOrExpr::VarDecl(_) => {}
                    }
                }
                if let Some(test) = &for_stmt.test {
                    collect_from_expr(test, out);
                }
                if let Some(update) = &for_stmt.update {
                    collect_from_expr(update, out);
                }
                collect_from_stmt(&for_stmt.body, out);
            }
            ast::Stmt::ForIn(for_in) => {
                collect_from_expr(&for_in.right, out);
                collect_from_stmt(&for_in.body, out);
            }
            ast::Stmt::ForOf(for_of) => {
                collect_from_expr(&for_of.right, out);
                collect_from_stmt(&for_of.body, out);
            }
            ast::Stmt::Labeled(labeled) => collect_from_stmt(&labeled.body, out),
            ast::Stmt::Switch(switch_stmt) => {
                collect_from_expr(&switch_stmt.discriminant, out);
                for case in &switch_stmt.cases {
                    if let Some(test) = &case.test {
                        collect_from_expr(test, out);
                    }
                    for stmt in &case.cons {
                        collect_from_stmt(stmt, out);
                    }
                }
            }
            ast::Stmt::Try(try_stmt) => {
                for stmt in &try_stmt.block.stmts {
                    collect_from_stmt(stmt, out);
                }
                if let Some(handler) = &try_stmt.handler {
                    for stmt in &handler.body.stmts {
                        collect_from_stmt(stmt, out);
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for stmt in &finalizer.stmts {
                        collect_from_stmt(stmt, out);
                    }
                }
            }
            ast::Stmt::Return(ret) => {
                if let Some(arg) = &ret.arg {
                    collect_from_expr(arg, out);
                }
            }
            ast::Stmt::Throw(throw_stmt) => collect_from_expr(&throw_stmt.arg, out),
            ast::Stmt::Decl(_)
            | ast::Stmt::Break(_)
            | ast::Stmt::Continue(_)
            | ast::Stmt::Debugger(_)
            | ast::Stmt::Empty(_) => {}
            _ => {}
        }
    }

    fn collect_from_expr(expr: &ast::Expr, out: &mut HashSet<String>) {
        match expr {
            ast::Expr::Assign(assign) => {
                if let ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) =
                    &assign.left
                {
                    out.insert(ident.id.sym.to_string());
                }
                collect_from_expr(&assign.right, out);
            }
            ast::Expr::Paren(paren) => collect_from_expr(&paren.expr, out),
            ast::Expr::Seq(seq) => {
                for expr in &seq.exprs {
                    collect_from_expr(expr, out);
                }
            }
            ast::Expr::Cond(cond) => {
                collect_from_expr(&cond.test, out);
                collect_from_expr(&cond.cons, out);
                collect_from_expr(&cond.alt, out);
            }
            ast::Expr::Bin(bin) => {
                collect_from_expr(&bin.left, out);
                collect_from_expr(&bin.right, out);
            }
            ast::Expr::Unary(unary) => collect_from_expr(&unary.arg, out),
            ast::Expr::Update(update) => {
                if let ast::Expr::Ident(ident) = update.arg.as_ref() {
                    out.insert(ident.sym.to_string());
                }
            }
            ast::Expr::Call(call) => {
                if let ast::Callee::Expr(callee) = &call.callee {
                    collect_from_expr(callee, out);
                }
                for arg in &call.args {
                    collect_from_expr(&arg.expr, out);
                }
            }
            ast::Expr::New(new_expr) => {
                collect_from_expr(&new_expr.callee, out);
                if let Some(args) = &new_expr.args {
                    for arg in args {
                        collect_from_expr(&arg.expr, out);
                    }
                }
            }
            ast::Expr::Member(member) => {
                collect_from_expr(&member.obj, out);
                if let ast::MemberProp::Computed(computed) = &member.prop {
                    collect_from_expr(&computed.expr, out);
                }
            }
            ast::Expr::TsAs(ts_as) => collect_from_expr(&ts_as.expr, out),
            ast::Expr::TsNonNull(ts_non_null) => collect_from_expr(&ts_non_null.expr, out),
            ast::Expr::TsTypeAssertion(ts_assert) => collect_from_expr(&ts_assert.expr, out),
            ast::Expr::TsSatisfies(ts_satisfies) => collect_from_expr(&ts_satisfies.expr, out),
            ast::Expr::TsConstAssertion(ts_const) => collect_from_expr(&ts_const.expr, out),
            _ => {}
        }
    }

    let mut out = HashSet::new();
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => collect_from_stmt(stmt, &mut out),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultExpr(default_expr)) => {
                collect_from_expr(&default_expr.expr, &mut out);
            }
            _ => {}
        }
    }
    out
}

pub fn lower_module(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
) -> Result<Module> {
    lower_module_with_class_id(ast_module, name, source_file_path, 1).map(|(module, _)| module)
}

pub fn lower_module_with_class_id(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_and_types(ast_module, name, source_file_path, start_class_id, None)
}

pub fn lower_module_with_class_id_and_types(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_types_and_seed(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        None,
    )
}

pub fn lower_module_with_class_id_types_and_seed(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
    imported_class_fields: Option<&std::collections::HashMap<String, Vec<(String, Type)>>>,
) -> Result<(Module, ClassId)> {
    lower_module_with_class_id_types_seed_and_entry(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        imported_class_fields,
        false,
    )
}

/// Issue #444: variant that takes `is_entry_module` so `import.meta.main`
/// resolves to `true` only inside the user-supplied entry TypeScript file
/// (matching Node 24+ / Bun semantics). All other lowering callers go
/// through the wrapper above with `is_entry_module=false`.
pub fn lower_module_with_class_id_types_seed_and_entry(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
    imported_class_fields: Option<&std::collections::HashMap<String, Vec<(String, Type)>>>,
    is_entry_module: bool,
) -> Result<(Module, ClassId)> {
    lower_module_full(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        imported_class_fields,
        is_entry_module,
        false,
    )
}

/// Issue #668: superset of the `_seed_and_entry` wrapper that also accepts
/// `is_external_module`. Callers in `crates/perry/src/commands/compile/`
/// pass `true` when the source file lives under any `node_modules/` segment
/// so the require-literal compile error in `lower_call.rs` skips library
/// code (which legitimately uses `require()` for deferred cycle breaks).
pub fn lower_module_full(
    ast_module: &ast::Module,
    name: &str,
    source_file_path: &str,
    start_class_id: ClassId,
    resolved_types: Option<std::collections::HashMap<u32, Type>>,
    imported_class_fields: Option<&std::collections::HashMap<String, Vec<(String, Type)>>>,
    is_entry_module: bool,
    is_external_module: bool,
) -> Result<(Module, ClassId)> {
    let mut ctx = LoweringContext::with_class_id_start(source_file_path, start_class_id);
    ctx.resolved_types = resolved_types;
    ctx.is_entry_module = is_entry_module;
    ctx.is_external_module = is_external_module;
    ctx.current_strict = module_has_strict_mode(ast_module);
    if let Some(seed) = imported_class_fields {
        ctx.seed_imported_class_fields(seed);
    }
    let mut module = Module::new(name);

    // Pre-scan for WeakRef/FinalizationRegistry variable declarations so subsequent
    // method-call lowering (`x.deref()`, `x.register(...)`, `x.unregister(...)`) can
    // route via the dedicated HIR variants without relying on type inference.
    pre_scan_weakref_locals(ast_module, &mut ctx);

    // Pre-scan for mixin functions: a function whose body is exactly
    // `return class extends <param> { ... };`. Lets `const Mixed = MixinFn(SomeClass)`
    // synthesize a real concrete class extending `SomeClass`.
    pre_scan_mixin_functions(ast_module, &mut ctx);

    // JSX expressions lower directly to the built-in `jsx`/`jsxs` externs in
    // `jsx.rs`. Do not synthesize a `react/jsx-runtime` import here: codegen
    // routes those extern names to Perry's runtime adapter, and making the
    // module graph resolve a fake package only produces a misleading warning.

    // Pre-scan: Find all function names that have implementations (bodies)
    // This is needed to properly handle TypeScript function overloads where
    // multiple signature-only declarations precede a single implementation
    let mut functions_with_bodies: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for item in &ast_module.body {
        let fn_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => Some(fn_decl),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Fn(fn_decl) = &export_decl.decl {
                    Some(fn_decl)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(fn_decl) = fn_decl {
            if fn_decl.function.body.is_some() {
                functions_with_bodies.insert(fn_decl.ident.sym.to_string());
            }
        }
    }

    // First pass: collect all function declarations (both exported and non-exported)
    // Skip 'declare function' statements (functions with no body) - they are external FFI
    // BUT: also skip overload signatures if an implementation exists
    let reassigned_function_candidates = collect_assigned_function_binding_candidates(ast_module);
    for item in &ast_module.body {
        // Extract function declaration from both regular statements and export declarations
        let fn_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) => Some(fn_decl),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Fn(fn_decl) = &export_decl.decl {
                    Some(fn_decl)
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(fn_decl) = fn_decl {
            let func_name = fn_decl.ident.sym.to_string();

            // Skip signature-only declarations (no body)
            if fn_decl.function.body.is_none() {
                // If this function has an implementation elsewhere, skip the signature
                // (it's a TypeScript overload, not an external FFI declaration)
                if functions_with_bodies.contains(&func_name) {
                    continue;
                }

                // No implementation exists - treat as external FFI declaration
                // Extract parameter types for FFI signature
                let param_types: Vec<Type> = fn_decl
                    .function
                    .params
                    .iter()
                    .map(|param| extract_param_type_with_ctx(&param.pat, None))
                    .collect();

                // Extract return type
                let return_type = fn_decl
                    .function
                    .return_type
                    .as_ref()
                    .map(|rt| extract_ts_type(&rt.type_ann))
                    .unwrap_or(Type::Void);

                // Register as external function so calls resolve to ExternFuncRef
                ctx.register_imported_func(func_name.clone(), func_name.clone());
                // Also store type information for code generation
                ctx.register_extern_func_types(func_name, param_types, return_type);
                continue;
            }

            // Function has a body - each declaration gets a unique FuncId
            // (inner-scope functions shadow outer-scope same-name functions via reverse lookup)
            let func_id = ctx.fresh_func();
            ctx.register_func(func_name.clone(), func_id);
            if reassigned_function_candidates.contains(&func_name)
                && ctx.lookup_local(&func_name).is_none()
            {
                let local_id = ctx.define_local(func_name.clone(), Type::Any);
                ctx.function_valued_locals.insert(local_id);
                module.init.push(Stmt::Let {
                    id: local_id,
                    name: func_name.clone(),
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::FuncRef(func_id)),
                });
            }

            // Pre-register return type annotation for call-site type inference
            // (so variables initialized from function calls can infer their type)
            if let Some(rt) = &fn_decl.function.return_type {
                let return_type = extract_ts_type(&rt.type_ann);
                if !matches!(return_type, Type::Any) {
                    ctx.register_func_return_type(func_name, return_type);
                }
            }
        }
    }

    // Pre-register module-level variable declarations so function bodies
    // declared before the variable can still reference them via lookup_local
    for item in &ast_module.body {
        let var_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(v))) => Some(v),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(v) = &export_decl.decl {
                    Some(v)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(var_decl) = var_decl {
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
    }

    // Pre-register all class declarations so that static method calls between
    // classes declared in the same file resolve correctly regardless of declaration order.
    // Without this, SqrtPriceMath.getAmount0Delta calling FullMath.mulDivRoundingUp
    // fails if FullMath is declared after SqrtPriceMath.
    for item in &ast_module.body {
        let class_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Class(cd))) => Some(cd),
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Class(cd) = &export_decl.decl {
                    Some(cd)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(cd) = class_decl {
            let name = cd.ident.sym.to_string();
            if ctx.lookup_class(&name).is_none() {
                let id = ctx.fresh_class();
                ctx.register_class(name.clone(), id);
            }
            // Collect static field/method names
            let mut static_field_names = Vec::new();
            let mut static_method_names = Vec::new();
            for member in &cd.class.body {
                match member {
                    ast::ClassMember::Method(method) if method.is_static => {
                        if let ast::PropName::Ident(ident) = &method.key {
                            static_method_names.push(ident.sym.to_string());
                        }
                    }
                    ast::ClassMember::PrivateMethod(method) if method.is_static => {
                        static_method_names.push(format!("#{}", method.key.name));
                    }
                    ast::ClassMember::ClassProp(prop) if prop.is_static => {
                        if let ast::PropName::Ident(ident) = &prop.key {
                            static_field_names.push(ident.sym.to_string());
                        }
                    }
                    ast::ClassMember::PrivateProp(prop) if prop.is_static => {
                        static_field_names.push(format!("#{}", prop.key.name));
                    }
                    _ => {}
                }
            }
            if !static_field_names.is_empty() || !static_method_names.is_empty() {
                // Only register if not already registered (lower_class_decl will re-register)
                if !ctx.class_statics.iter().any(|(cn, _, _)| cn == &name) {
                    ctx.register_class_statics(name, static_field_names, static_method_names);
                }
            }
        }
    }

    // Main pass: lower everything
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(stmt) => {
                lower_stmt(&mut ctx, &mut module, stmt)?;
            }
            ast::ModuleItem::ModuleDecl(decl) => {
                lower_module_decl(&mut ctx, &mut module, decl)?;
            }
        }
        // Flush any pending functions created during expression lowering
        // (e.g., inline methods in object literals)
        for func in ctx.pending_functions.drain(..) {
            module.functions.push(func);
        }
        // Flush #2076 display-name overrides recorded for named fn
        // expressions and object-literal methods.
        for (id, name) in ctx.closure_display_names.drain() {
            module.closure_display_names.insert(id, name);
        }
        // Flush any pending classes created during expression lowering
        // (e.g., class expressions in `new (class extends Command { ... })()`)
        for class in ctx.pending_classes.drain(..) {
            push_class_dedup(&mut module, class);
        }
    }

    // Populate exported_native_instances by matching native_instances with exports
    for (local_name, module_name, class_name) in &ctx.native_instances {
        // Check if this native instance is exported
        for export in &module.exports {
            if let Export::Named { local, exported } = export {
                if local == local_name {
                    module.exported_native_instances.push((
                        exported.clone(),
                        module_name.clone(),
                        class_name.clone(),
                    ));
                }
            }
        }
    }

    // Populate exported_func_return_native_instances for functions that return native instances
    for (func_name, native_module, native_class) in &ctx.func_return_native_instances {
        // Check if this function is directly exported
        let is_exported = module
            .functions
            .iter()
            .any(|f| f.name == *func_name && f.is_exported);
        if is_exported {
            module.exported_func_return_native_instances.push((
                func_name.clone(),
                native_module.clone(),
                native_class.clone(),
            ));
        } else {
            // Also check named exports (e.g., `export { getRedis }`)
            for export in &module.exports {
                if let Export::Named { local, exported } = export {
                    if local == func_name {
                        module.exported_func_return_native_instances.push((
                            exported.clone(),
                            native_module.clone(),
                            native_class.clone(),
                        ));
                    }
                }
            }
        }
    }

    module.uses_fetch = ctx.uses_fetch;
    module.uses_webassembly = ctx.uses_webassembly;
    module.extern_funcs = ctx.extern_func_types.clone();

    // Post-pass: widen `mutable_captures` across sibling closures. When two
    // closures in the same scope share a capture and one of them assigns to
    // it, the variable must be boxed; every closure that captures it must
    // also go through the box so they observe each other's writes. Without
    // this pass, a `get: () => value` sibling of `inc: () => value++` captures
    // the raw initial value instead of the shared boxed binding.
    widen_mutable_captures_stmts(&mut module.init);
    for func in &mut module.functions {
        widen_mutable_captures_stmts(&mut func.body);
    }
    for class in &mut module.classes {
        for method in &mut class.methods {
            widen_mutable_captures_stmts(&mut method.body);
        }
        for (_, getter) in &mut class.getters {
            widen_mutable_captures_stmts(&mut getter.body);
        }
        for (_, setter) in &mut class.setters {
            widen_mutable_captures_stmts(&mut setter.body);
        }
        for static_method in &mut class.static_methods {
            widen_mutable_captures_stmts(&mut static_method.body);
        }
        if let Some(ref mut ctor) = class.constructor {
            widen_mutable_captures_stmts(&mut ctor.body);
        }
    }

    // Post-pass: infer `extends_name` from `extends_expr` for the bare-factory
    // shape `class Sub extends makeFactory() {}` where `makeFactory` is a
    // top-level function whose body trivially returns a static `ClassRef`.
    // Without this, the codegen chain walks
    // (`apply_field_initializers_recursive` + the keys-array generator) walk
    // by `extends_name` only, see `None`, and skip the factory class's
    // field initializers entirely — `new Sub().kind` reads `undefined`
    // instead of the parent's `kind = "bare"` literal. Surfaced by the
    // #806 mixin harness (bare-factory section).
    infer_dynamic_extends_names(&mut module);

    Ok((module, ctx.next_class_id))
}
