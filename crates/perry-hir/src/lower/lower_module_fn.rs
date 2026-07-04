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

fn module_has_strict_mode(ast_module: &ast::Module) -> bool {
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::ModuleDecl(_) => return true,
            ast::ModuleItem::Stmt(stmt) => {
                let Some(directive) = string_directive_stmt_lit(stmt) else {
                    break;
                };
                if is_raw_use_strict_directive(directive) {
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
            ast::Stmt::Decl(ast::Decl::Var(var_decl)) => {
                for decl in &var_decl.decls {
                    if let Some(init) = &decl.init {
                        collect_from_expr(init, out);
                    }
                }
            }
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
                    let name = ident.id.sym.as_ref();
                    let is_self_read = assign.op == ast::AssignOp::Assign
                        && matches!(
                            assign.right.as_ref(),
                            ast::Expr::Ident(rhs) if rhs.sym.as_ref() == name
                        );
                    if !is_self_read {
                        out.insert(name.to_string());
                    }
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
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Var(var_decl) = &export_decl.decl {
                    for decl in &var_decl.decls {
                        if let Some(init) = &decl.init {
                            collect_from_expr(init, &mut out);
                        }
                    }
                }
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultExpr(default_expr)) => {
                collect_from_expr(&default_expr.expr, &mut out);
            }
            _ => {}
        }
    }
    out
}

/// #5833: names assigned by a **direct top-level** `name = ...`/`name++`
/// expression statement (module scope, not nested in any block/if/loop/
/// function). Deliberately narrower than
/// [`collect_assigned_function_binding_candidates`] — that scan is name-only
/// (no scope tracking), so reusing it for the class-reassignment gate below
/// would false-positive on a same-named binding shadowed in a nested block
/// (`class Foo {}; { let Foo; Foo = 1; }` would wrongly mark the top-level
/// `Foo` reassigned) and silently miss a reassignment inside a nested
/// function body (which the deep scan doesn't walk into either). Scoping to
/// only the shapes a genuine top-level class-binding reassignment can take
/// avoids the false positive entirely; the false negative (reassigning a
/// top-level class from inside a block or a nested function) simply falls
/// back to the pre-existing behavior (the assignment is silently dropped,
/// same as before this fix) rather than being newly and only partially
/// correct.
fn collect_direct_top_level_reassigned_identifiers(ast_module: &ast::Module) -> HashSet<String> {
    fn target_name(target: &ast::AssignTarget) -> Option<&str> {
        match target {
            ast::AssignTarget::Simple(ast::SimpleAssignTarget::Ident(ident)) => {
                Some(ident.id.sym.as_ref())
            }
            _ => None,
        }
    }

    let mut out = HashSet::new();
    for item in &ast_module.body {
        let ast::ModuleItem::Stmt(ast::Stmt::Expr(expr_stmt)) = item else {
            continue;
        };
        match expr_stmt.expr.as_ref() {
            ast::Expr::Assign(assign) => {
                if let Some(name) = target_name(&assign.left) {
                    out.insert(name.to_string());
                }
            }
            ast::Expr::Update(update) => {
                if let ast::Expr::Ident(ident) = update.arg.as_ref() {
                    out.insert(ident.sym.to_string());
                }
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
        None,
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
    imported_class_accessors: Option<&std::collections::HashMap<String, crate::ClassAccessorNames>>,
    is_entry_module: bool,
) -> Result<(Module, ClassId)> {
    lower_module_full(
        ast_module,
        name,
        source_file_path,
        start_class_id,
        resolved_types,
        imported_class_fields,
        imported_class_accessors,
        is_entry_module,
        false,
    )
}

/// #4461: true when a `var/let/const X = class { ... }` declarator binds an
/// identifier to a class *expression* (unwrapping `paren`/`as`/`!`/type-
/// assertion layers that esbuild/rollup dist bundles emit). Such bindings are
/// lowered to a class named after the binding in `stmt.rs`, so they must not be
/// pre-registered as module-level locals (which would shadow the class ref).
fn decl_init_is_class_expr(decl: &ast::VarDeclarator) -> bool {
    if !matches!(decl.name, ast::Pat::Ident(_)) {
        return false;
    }
    let mut e = match &decl.init {
        Some(init) => init.as_ref(),
        None => return false,
    };
    loop {
        match e {
            ast::Expr::Paren(p) => e = &p.expr,
            ast::Expr::TsAs(a) => e = &a.expr,
            ast::Expr::TsNonNull(n) => e = &n.expr,
            ast::Expr::TsTypeAssertion(a) => e = &a.expr,
            ast::Expr::Class(_) => return true,
            _ => return false,
        }
    }
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
    imported_class_accessors: Option<&std::collections::HashMap<String, crate::ClassAccessorNames>>,
    is_entry_module: bool,
    is_external_module: bool,
) -> Result<(Module, ClassId)> {
    let mut ctx = LoweringContext::with_class_id_start(source_file_path, start_class_id);
    ctx.resolved_types = resolved_types;
    ctx.is_entry_module = is_entry_module;
    ctx.is_external_module = is_external_module;
    ctx.module_strict = module_has_strict_mode(ast_module);
    ctx.current_strict = ctx.module_strict;
    if let Some(seed) = imported_class_fields {
        ctx.seed_imported_class_fields(seed);
    }
    if let Some(seed) = imported_class_accessors {
        ctx.seed_imported_class_accessors(seed);
    }
    let mut module = Module::new(name);

    // Pre-scan for `new Function` / `Function(...)` constant-argument
    // resolution: single-assignment module vars, `toString`-bearing object
    // literals, and counter vars (see `fn_ctor_env`).
    ctx.fn_ctor_env = super::fn_ctor_env::build_fn_ctor_env(ast_module);

    // Pre-scan for WeakRef/FinalizationRegistry variable declarations so subsequent
    // method-call lowering (`x.deref()`, `x.register(...)`, `x.unregister(...)`) can
    // route via the dedicated HIR variants without relying on type inference.
    pre_scan_weakref_locals(ast_module, &mut ctx);

    // Pre-scan for mixin functions: a function whose body is exactly
    // `return class extends <param> { ... };`. Lets `const Mixed = MixinFn(SomeClass)`
    // synthesize a real concrete class extending `SomeClass`.
    pre_scan_mixin_functions(ast_module, &mut ctx);

    // #4510: register module-level enums up front so a function body (or any
    // statement) that references an enum declared later in the file resolves
    // the member instead of silently lowering to 0.
    pre_register_module_enums(ast_module, &mut ctx);

    // Propagate a native host-handle's class across direct function-call
    // boundaries (the `("ws","Client")` upgrade `wsId` handed to a helper) so
    // `wsId.send(...)` inside the callee dispatches to the Client runtime
    // instead of a silent generic no-op. Must run before any function body is
    // lowered so `lower_fn_decl` can tag the receiving parameter.
    pre_scan_cross_fn_native_params(ast_module, &mut ctx);

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
    // #5833: a narrower, binding-aware-enough scan stashed on `ctx` so
    // `stmt.rs`'s top-level `Decl::Class` arm can gate its opt-in local-slot
    // seeding (see both `reassigned_top_level_identifiers`'s doc comment and
    // `collect_direct_top_level_reassigned_identifiers`'s).
    ctx.reassigned_top_level_identifiers =
        collect_direct_top_level_reassigned_identifiers(ast_module);
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

    // #5134: a *named* `export default function foo() {}` also introduces a
    // hoisted `foo` binding in module scope — usable for self-recursion and
    // same-module references, exactly like a plain `function foo`. The earlier
    // loop only handles `Stmt::Decl(Fn)` / `export function`, so without this
    // the name went unregistered and references to it (e.g. ramda's
    // `_curryN` calling itself inside its returned closure) lowered to an
    // unresolved global → `ReferenceError: _curryN is not defined`. The
    // dedicated lowering in `module_decl.rs` reuses this pre-registered id
    // (`lower_fn_decl`: `lookup_func(name).unwrap_or_else(fresh_func)`).
    for item in &ast_module.body {
        if let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultDecl(export_default)) =
            item
        {
            if let ast::DefaultDecl::Fn(fn_expr) = &export_default.decl {
                if let Some(ident) = &fn_expr.ident {
                    if fn_expr.function.body.is_some() {
                        let func_name = ident.sym.to_string();
                        if ctx.lookup_func(&func_name).is_none() {
                            let func_id = ctx.fresh_func();
                            ctx.register_func(func_name, func_id);
                        }
                    }
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
                // #4461: `var X = class { ... }` is lowered as a class
                // expression bound to the name `X` (see stmt.rs) — the class
                // itself takes the role of the value referenced by name, and
                // no `Stmt::Let` is ever emitted for `X`. Pre-registering `X`
                // as a module-level local here would shadow that class with a
                // never-assigned local, so a value read of `X` (`typeof X`,
                // `X.staticMethod`, passing `X` around) resolved to the
                // undefined local instead of the class ref. Skip the local so
                // `Ident("X")` lowers to `Expr::ClassRef("X")`.
                if decl_init_is_class_expr(decl) {
                    continue;
                }
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
                        if var_decl.kind == ast::VarDeclKind::Var {
                            ctx.pre_registered_module_var_decls
                                .insert(ident.id.sym.to_string());
                        }
                    }
                } else if matches!(&decl.name, ast::Pat::Object(_) | ast::Pat::Array(_)) {
                    // #5358: a module-level DESTRUCTURING binding
                    // (`const { src, t } = require('./re.js')`) declared after
                    // code that references those names — the canonical CJS
                    // "require at the bottom for cyclic deps" pattern — must
                    // pre-register each destructured leaf so a class/function
                    // body lowered earlier resolves `src`/`t` to the module
                    // slot, not an undefined implicit global. Without this the
                    // destructuring leaf later allocates a *fresh* id and the
                    // earlier reference points at the wrong (undefined) slot.
                    // (The simple-ident arm above already handles `const x =`.)
                    let mut leaf_names = Vec::new();
                    crate::lower_patterns::collect_binding_names(&decl.name, &mut leaf_names);
                    for name in leaf_names {
                        if ctx.lookup_local(&name).is_none() {
                            ctx.define_local(name.clone(), Type::Any);
                            ctx.pre_registered_module_vars.insert(name.clone());
                            if var_decl.kind == ast::VarDeclKind::Var {
                                ctx.pre_registered_module_var_decls.insert(name);
                            }
                        }
                    }
                }
            }
        }
    }

    // Pre-register `var` bindings nested inside module-level blocks, loops,
    // try/catch, switch and with statements. `var` is function/module-scoped,
    // so `__x = __x` before `try { var __x; }`, or a read of `foo` after
    // `try { ... } catch (e) { var foo = 1; }`, must resolve to one hoisted
    // module binding (initialised to undefined) rather than an implicit-global
    // lookup that throws ReferenceError at runtime. The ids go into
    // `var_hoisted_ids` so the nested `Stmt::Let` reuses them (see the
    // `is_var_decl` reuse path in destructuring/var_decl.rs) and block-scope
    // pops preserve them.
    for item in &ast_module.body {
        let stmt = match item {
            ast::ModuleItem::Stmt(stmt) => stmt,
            _ => continue,
        };
        // Direct top-level var decls are handled by the pass above; only
        // walk into compound statements for nested `var`s here.
        if matches!(stmt, ast::Stmt::Decl(_)) {
            continue;
        }
        let mut names = Vec::new();
        crate::lower_decl::collect_var_binding_names_from_stmt(stmt, &mut names);
        names.sort();
        names.dedup();
        for name in names {
            if ctx.lookup_local(&name).is_none() {
                let id = ctx.define_local(name.clone(), Type::Any);
                ctx.var_hoisted_ids.insert(id);
                // Emit an explicit undefined-initialised slot at the top of
                // module init. Codegen creates local storage at the first
                // `Stmt::Let` it sees for an id; without this, a read
                // compiled before the nested decl (e.g. `if (c) break;`
                // ahead of `var c = ...` inside the same loop body) bakes
                // in an `undefined` constant and never observes the write.
                // The nested `Stmt::Let` later reuses this slot via the
                // redeclaration → LocalSet path in codegen's lower_let.
                module.init.push(Stmt::Let {
                    id,
                    name,
                    ty: Type::Any,
                    mutable: true,
                    init: Some(Expr::Undefined),
                });
            }
        }
    }

    // Annex B B.3.3 (#5297): in sloppy (non-strict) global code, a block-nested
    // `function f(){}` also creates a global `var f` (undefined until the
    // declaration runs). Mirror the function-body pre-pass: register one hoisted
    // slot per such name, emit an undefined-initialised entry, and record name
    // -> slot in `annexb_block_fn_var_ids` so the block-nested declaration
    // (lowered via `lower_nested_fn_decl`) writes the closure into it while
    // keeping its block-local binding independent.
    if !ctx.module_strict {
        let body_stmts: Vec<ast::Stmt> = ast_module
            .body
            .iter()
            .filter_map(|item| match item {
                ast::ModuleItem::Stmt(stmt) => Some(stmt.clone()),
                _ => None,
            })
            .collect();
        // Forbidden: the program's own top-level lexical names make `var f` an
        // early error; `arguments` is excluded. There are no parameters at
        // program scope. Nested blocks add their own lexical names while
        // descending.
        let mut forbidden = std::collections::HashSet::new();
        crate::lower_decl::collect_lexical_decl_names(&body_stmts, &mut forbidden);
        forbidden.insert("arguments".to_string());

        let mut all_names = Vec::new();
        let mut names = Vec::new();
        crate::lower_decl::collect_annexb_block_fn_decl_names(
            &body_stmts,
            &forbidden,
            &mut all_names,
            &mut names,
        );
        ctx.annexb_block_fn_names_all.extend(all_names);
        names.sort();
        names.dedup();
        for name in names {
            // Reuse an existing global `var`, else mint a fresh hoisted slot;
            // either way emit an entry slot so the block's B.3.3 write (which
            // runs before any source-position `var f = …`) has storage to target.
            let id = if let Some(existing) = ctx.lookup_local(&name) {
                existing
            } else {
                ctx.define_local(name.clone(), Type::Any)
            };
            // B.3.3 entry value. Normally the legacy `var` is `undefined` until
            // the block declaration runs. But when a same-named *top-level*
            // function declaration also exists, F is already in
            // declaredFunctionNames, so B.3.3 does NOT create a fresh
            // `undefined` binding — the function declaration owns the entry
            // value. A non-reassigned top-level `function f` is otherwise called
            // straight through `lookup_func` and never bound to this var slot,
            // so without this the legacy var shadows it as `undefined` and
            // `f()` throws at entry (the `existing-fn-no-init` cluster, #5346).
            // Seed the slot with the function and mark it function-valued; the
            // block-level declaration still overwrites it (`existing-fn-update`).
            let init = match ctx.lookup_func(&name) {
                Some(func_id) if functions_with_bodies.contains(&name) => {
                    ctx.function_valued_locals.insert(id);
                    Expr::FuncRef(func_id)
                }
                _ => Expr::Undefined,
            };
            // #5848: no same-named bare top-level function decl covers this
            // name (that case is already reflected onto `globalThis` with its
            // real value via `script_global_functions`) — record it so codegen
            // seeds an early `undefined`-valued, non-configurable global
            // property (GlobalDeclarationInstantiation's `CreateGlobalVarBinding`
            // runs before any top-level statement executes).
            if matches!(init, Expr::Undefined) {
                module.annexb_global_undefined_names.push(name.clone());
            }
            module.init.push(Stmt::Let {
                id,
                name: name.clone(),
                ty: Type::Any,
                mutable: true,
                init: Some(init),
            });
            ctx.var_hoisted_ids.insert(id);
            ctx.annexb_block_fn_var_ids.insert(name, id);
        }
    }

    // Pre-register all class declarations so that static method calls between
    // classes declared in the same file resolve correctly regardless of declaration order.
    // Without this, SqrtPriceMath.getAmount0Delta calling FullMath.mulDivRoundingUp
    // fails if FullMath is declared after SqrtPriceMath.
    for item in &ast_module.body {
        let class_decl = match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Class(cd))) => {
                Some((cd.ident.sym.to_string(), &cd.class))
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export_decl)) => {
                if let ast::Decl::Class(cd) = &export_decl.decl {
                    Some((cd.ident.sym.to_string(), &cd.class))
                } else {
                    None
                }
            }
            // #4976: named inline `export default class Name { … }` is a
            // real class declaration too — pre-register it so same-file
            // static cross-references resolve regardless of order.
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultDecl(
                ast::ExportDefaultDecl {
                    decl: ast::DefaultDecl::Class(class_expr),
                    ..
                },
            )) => class_expr
                .ident
                .as_ref()
                .map(|ident| (ident.sym.to_string(), &class_expr.class)),
            _ => None,
        };
        if let Some((name, cd)) = class_decl {
            // Record this as a real top-level class DECLARATION so a
            // same-named nested class EXPRESSION (minimatch's
            // `defaults()` → `{ Minimatch: class Minimatch extends … }`)
            // doesn't hijack its ClassId in `lower_class_from_ast`.
            ctx.module_class_decl_names.insert(name.clone());
            if ctx.lookup_class(&name).is_none() {
                let id = ctx.fresh_class();
                ctx.register_class(name.clone(), id);
            }
            // Collect static field/method names
            let mut static_field_names = Vec::new();
            let mut static_method_names = Vec::new();
            for member in &cd.body {
                match member {
                    // Only true static *methods* register as callable statics.
                    // Static accessors (`static get foo()`) are NOT methods —
                    // `C.foo(...)` must read the accessor (invoking the getter)
                    // and call its result, not dispatch a static method named
                    // `foo`. Registering them here makes `has_static_method`
                    // hijack the call into a StaticMethodCall whose target
                    // doesn't exist, silently dropping the call. Refs test262
                    // language/arguments-object cls-*-static-* getter calls.
                    ast::ClassMember::Method(method)
                        if method.is_static && matches!(method.kind, ast::MethodKind::Method) =>
                    {
                        if let ast::PropName::Ident(ident) = &method.key {
                            static_method_names.push(ident.sym.to_string());
                        }
                    }
                    ast::ClassMember::PrivateMethod(method)
                        if method.is_static && matches!(method.kind, ast::MethodKind::Method) =>
                    {
                        static_method_names.push(format!("#{}", method.key.name));
                    }
                    ast::ClassMember::ClassProp(prop) if prop.is_static && !prop.declare => {
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
        // #5592: flush class `.name` overrides for uniquified class-expression
        // registration keys.
        for (id, name) in ctx.class_display_names.drain() {
            module.class_display_names.insert(id, name);
        }
        // Flush generator param-prologue lengths (run param binding at call time).
        for (id, len) in ctx.gen_param_prologue_len.drain() {
            module.gen_param_prologue_len.insert(id, len);
        }
        // #4101: flush captured function source text for `fn.toString()`.
        for (id, src) in ctx.closure_source_text.drain() {
            module.closure_source_text.insert(id, src);
        }
        // Flush any pending classes created during expression lowering
        // (e.g., class expressions in `new (class extends Command { ... })()`)
        for class in ctx.pending_classes.drain(..) {
            push_class_dedup(&mut module, class);
        }
    }

    // #5579: record whether the source references `globalThis`, gating the
    // codegen reflection of top-level `function` declarations onto the global
    // object (see `Module::references_global_this`). The module source is
    // installed for the duration of this lower (collect_modules.rs).
    // #5833: OR in `ctx.saw_global_this_expr` — a top-level `this` read in
    // global-script mode is the same global-object reference as the literal
    // `globalThis` token, but the substring scan above can't see it.
    module.references_global_this =
        crate::ir::current_module_source_mentions_global_this() || ctx.saw_global_this_expr;

    // #5833: GlobalDeclarationInstantiation step 5c — a top-level `let`/
    // `const`/`class` declaration whose name collides with a "restricted
    // global property" (HasRestrictedGlobalProperty) is an early SyntaxError,
    // thrown before any statement runs. Only `undefined`, `NaN`, and
    // `Infinity` are non-configurable value properties of a pristine global
    // object (ECMA-262 §19.1.1-19.1.3), so this is a purely static check
    // against the entry module's own top-level lexical names — it doesn't
    // need to model the general (dynamically extensible) case. Scoped to the
    // ENTRY module compiled as a Script: an ES module (import/export syntax,
    // OR top-level `await` — matching `is_esm_entry` in
    // `perry-codegen/src/codegen/entry.rs` exactly) binds in its own Module
    // Environment Record instead, never touching the global object, and a
    // non-entry module never reaches GlobalDeclarationInstantiation
    // regardless of its own syntax. Runs here (after the main lowering pass,
    // not the earlier pre-pass) so `module.imports`/`module.exports` are
    // populated and `detect_top_level_await` — which needs the already-
    // lowered `module.init` HIR to tell a real top-level `await` from one
    // nested in a closure's own async scope — has something to scan. Test262
    // `language/global-code/decl-lex-restricted-global.js`.
    crate::dynamic_import::detect_top_level_await(&mut module);
    let is_esm_entry =
        !module.imports.is_empty() || !module.exports.is_empty() || module.has_top_level_await;
    if ctx.is_entry_module && !is_esm_entry {
        const RESTRICTED_GLOBAL_NAMES: [&str; 3] = ["undefined", "NaN", "Infinity"];
        let restricted_scan_stmts: Vec<ast::Stmt> = ast_module
            .body
            .iter()
            .filter_map(|item| match item {
                ast::ModuleItem::Stmt(stmt) => Some(stmt.clone()),
                _ => None,
            })
            .collect();
        let mut top_level_lexical_names = std::collections::HashSet::new();
        crate::lower_decl::collect_lexical_decl_names(
            &restricted_scan_stmts,
            &mut top_level_lexical_names,
        );
        for name in RESTRICTED_GLOBAL_NAMES {
            if top_level_lexical_names.contains(name) {
                anyhow::bail!(
                    "SyntaxError: identifier '{name}' has already been declared \
                     (restricted global property)"
                );
            }
        }
    }

    if !ctx.sloppy_implicit_globals.is_empty() {
        let mut implicit_globals: Vec<Stmt> = ctx
            .sloppy_implicit_globals
            .iter()
            .map(|(name, id)| {
                // #5579: a `with`-set FALLBACK sloppy global (`with (o) { p1 =
                // 'x1'; }`) must hoist as the HOLE sentinel, NOT `undefined`.
                // The HOLE routes a later bare read through
                // `js_with_implicit_read`, which resolves the name against the
                // global object (so `globalThis.p1` set independently reads
                // back) and only throws when it is genuinely unresolvable. A
                // plain `undefined` hoist instead makes the read silently yield
                // `undefined`. This module-scope hoist is the ONLY declaration
                // site for the slot when the `with` is nested inside a function
                // (the in-body HOLE init lands in the callee's own frame), so
                // without this the function-nested cases regress
                // (S12.10_A1.2/A1.3/A3.2). Plain sloppy implicit globals
                // (`undeclared = 1` outside any `with`) keep the `undefined`
                // hoist.
                if ctx.with_sloppy_implicit_ids.contains_key(id) {
                    with_implicit_unset_let(*id, name.clone())
                } else {
                    Stmt::Let {
                        id: *id,
                        name: name.clone(),
                        ty: Type::Any,
                        mutable: true,
                        init: Some(Expr::Undefined),
                    }
                }
            })
            .collect();
        implicit_globals.append(&mut module.init);
        module.init = implicit_globals;
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

    // Post-pass: widen declared types lied about by later assignments
    // (`var x = 2; … set foo(v){ x = this; }` must not leave `x: Number`,
    // or codegen float-compares NaN-boxed pointers — #3576 family). Collect
    // over EVERY body first (LocalIds are module-unique; the assignment and
    // the `Stmt::Let` can live in different bodies), then rewrite.
    {
        let mut widening = crate::lower::type_widening::TypeWidening::from_module(&module);
        widening.collect(&module.init);
        for func in &module.functions {
            widening.collect(&func.body);
        }
        for class in &module.classes {
            for method in &class.methods {
                widening.collect_in_class(&class.name, &method.body);
            }
            // `getters`/`setters` hold both instance and static accessors
            // (static ones flagged in `static_accessor_fn_ids`). Static
            // accessors bind `this` to the constructor, not an instance, so
            // only instance accessors get instance-style `this`/`super` facts.
            for (_, getter) in &class.getters {
                if class.static_accessor_fn_ids.contains(&getter.id) {
                    widening.collect(&getter.body);
                } else {
                    widening.collect_in_class(&class.name, &getter.body);
                }
            }
            for (_, setter) in &class.setters {
                if class.static_accessor_fn_ids.contains(&setter.id) {
                    widening.collect(&setter.body);
                } else {
                    widening.collect_in_class(&class.name, &setter.body);
                }
            }
            // Static methods bind `this` to the constructor, not an instance,
            // so instance-member resolution would be wrong — keep it bare.
            for static_method in &class.static_methods {
                widening.collect(&static_method.body);
            }
            if let Some(ref ctor) = class.constructor {
                widening.collect_in_class(&class.name, &ctor.body);
            }
        }
        widening.apply(&mut module.init);
        for func in &mut module.functions {
            widening.apply(&mut func.body);
        }
        for class in &mut module.classes {
            for method in &mut class.methods {
                widening.apply(&mut method.body);
            }
            for (_, getter) in &mut class.getters {
                widening.apply(&mut getter.body);
            }
            for (_, setter) in &mut class.setters {
                widening.apply(&mut setter.body);
            }
            for static_method in &mut class.static_methods {
                widening.apply(&mut static_method.body);
            }
            if let Some(ref mut ctor) = class.constructor {
                widening.apply(&mut ctor.body);
            }
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
