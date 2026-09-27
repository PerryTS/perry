use super::CompilationContext;
use anyhow::{bail, Result};
use perry_hir::walker::walk_expr_children;
use perry_hir::{
    Class, Decorator, Export, Expr, Function, ImportSpecifier, Module, ModuleKind, Stmt,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Availability {
    Present,
    Absent,
    // Builtin/runtime-backed exports have no complete HIR export list.
    Unknown,
}

fn availability(
    ctx: &mut CompilationContext,
    path: &Path,
    name: &str,
    seen: &mut HashSet<(PathBuf, String)>,
) -> Availability {
    if !seen.insert((path.to_path_buf(), name.to_owned())) {
        return Availability::Absent;
    }
    let Some(module) = ctx.native_modules.get(path) else {
        return Availability::Unknown;
    };
    let exports = module.exports.clone();
    let mut result = Availability::Absent;
    for export in exports {
        let (source, imported) = match export {
            Export::Named { exported, .. } if exported == name => return Availability::Present,
            Export::NamespaceReExport { name: exported, .. } if exported == name => {
                return Availability::Present;
            }
            Export::ReExport {
                source,
                imported,
                exported,
            } if exported == name => (source, imported),
            Export::ExportAll { source } if name != "default" => (source, name.to_owned()),
            _ => continue,
        };
        let found = match super::super::cached_resolve_import(&source, path, ctx) {
            Some((target, ModuleKind::NativeCompiled)) => {
                availability(ctx, &target, &imported, seen)
            }
            _ => Availability::Unknown,
        };
        match found {
            Availability::Present => return found,
            Availability::Unknown => result = found,
            Availability::Absent => {}
        }
    }
    result
}

/// TypeScript elides an import specifier whose binding is never used as a
/// value: `import { type T }`, and just as often `import { Options }` where
/// `Options` is an interface, a type re-exported through `export type { .. }`,
/// or a name that exists only in a JavaScript package's `.d.ts` sidecar. None
/// of those has a runtime export for this check to find, and none of them
/// reaches the linker, so a TypeScript importer is only held to the names it
/// reads as values. JavaScript importers keep Node's static-ESM rule: every
/// named specifier must resolve, used or not.
fn is_typescript_importer(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "tsx" | "mts" | "cts")
    )
}

/// Reject statically absent named exports while module/source context is still
/// available, instead of inventing a perry_fn symbol that fails in the linker.
pub(super) fn enforce(ctx: &mut CompilationContext) -> Result<()> {
    let mut edges = Vec::new();
    let mut value_refs: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for (importer, module) in &ctx.native_modules {
        for import in &module.imports {
            if import.type_only
                || import.runtime_erased
                || import.is_dynamic
                || import.is_native
                || import.is_adopted_require
                || import.module_kind != ModuleKind::NativeCompiled
            {
                continue;
            }
            let Some(target) = &import.resolved_path else {
                continue;
            };
            for specifier in &import.specifiers {
                if let ImportSpecifier::Named { imported, local } = specifier {
                    if is_typescript_importer(importer)
                        && !value_refs
                            .entry(importer.clone())
                            .or_insert_with(|| value_reference_names(module))
                            .contains(local)
                    {
                        continue;
                    }
                    edges.push((
                        importer.clone(),
                        import.source.clone(),
                        target.clone(),
                        imported.clone(),
                        local.clone(),
                    ));
                }
            }
        }
    }
    edges.sort();
    for (importer, source, target, name, local) in edges {
        if availability(ctx, Path::new(&target), &name, &mut HashSet::new()) == Availability::Absent
        {
            bail!(
                "The requested module '{}' does not provide an export named '{}' \
                 (imported as '{}' in {}). Resolved module: {}. \
                 For CommonJS properties not exposed as named exports, use a default import and read the property from it.",
                source, name, local, importer.display(), target
            );
        }
    }
    Ok(())
}

/// Names that `module`'s executable code reads as runtime bindings: imported
/// functions/values (`ExternFuncRef`) and the class/enum-name forms lowering
/// uses for imported classes. Type annotations live in `Type`, never in an
/// `Expr`, so a type-position-only import never appears here. A reference
/// form missing from this list only means that import is not pre-checked and
/// falls back to the linker, never that a valid program is rejected.
pub(super) fn value_reference_names(module: &Module) -> HashSet<String> {
    let mut out = HashSet::new();
    visit_stmts(&module.init, &mut out);
    for function in &module.functions {
        visit_function(function, &mut out);
    }
    for class in &module.classes {
        visit_class(class, &mut out);
    }
    for global in &module.globals {
        if let Some(init) = &global.init {
            visit_expr(init, &mut out);
        }
    }
    out
}

fn visit_class(class: &Class, out: &mut HashSet<String>) {
    if let Some(parent) = &class.extends_name {
        out.insert(parent.clone());
    }
    if let Some(expr) = &class.extends_expr {
        visit_expr(expr, out);
    }
    visit_decorators(&class.decorators, out);
    if let Some(ctor) = &class.constructor {
        visit_function(ctor, out);
    }
    for function in class.methods.iter().chain(&class.static_methods) {
        visit_function(function, out);
    }
    for (_, function) in class.getters.iter().chain(&class.setters) {
        visit_function(function, out);
    }
    for member in &class.computed_members {
        visit_expr(&member.key_expr, out);
        visit_function(&member.function, out);
    }
    for field in class.fields.iter().chain(&class.static_fields) {
        visit_decorators(&field.decorators, out);
        for expr in field.key_expr.iter().chain(&field.init) {
            visit_expr(expr, out);
        }
    }
}

fn visit_decorators(decorators: &[Decorator], out: &mut HashSet<String>) {
    for decorator in decorators {
        out.insert(decorator.name.clone());
        for arg in &decorator.args {
            visit_expr(arg, out);
        }
    }
}

fn visit_function(function: &Function, out: &mut HashSet<String>) {
    visit_decorators(&function.decorators, out);
    for param in &function.params {
        visit_decorators(&param.decorators, out);
        if let Some(default) = &param.default {
            visit_expr(default, out);
        }
    }
    visit_stmts(&function.body, out);
}

fn visit_expr(expr: &Expr, out: &mut HashSet<String>) {
    match expr {
        Expr::ExternFuncRef { name, .. } | Expr::ClassRef(name) => {
            out.insert(name.clone());
        }
        Expr::New { class_name, .. }
        | Expr::JsNew { class_name, .. }
        | Expr::StaticFieldGet { class_name, .. }
        | Expr::StaticFieldSet { class_name, .. }
        | Expr::StaticMethodCall { class_name, .. } => {
            out.insert(class_name.clone());
        }
        Expr::InstanceOf { ty, .. } => {
            out.insert(ty.clone());
        }
        Expr::EnumMember { enum_name, .. } => {
            out.insert(enum_name.clone());
        }
        // `walk_expr_children` visits a closure's param defaults but not its
        // statement body.
        Expr::Closure { body, .. } => visit_stmts(body, out),
        _ => {}
    }
    walk_expr_children(expr, &mut |child| visit_expr(child, out));
}

fn visit_stmts(stmts: &[Stmt], out: &mut HashSet<String>) {
    for stmt in stmts {
        visit_stmt(stmt, out);
    }
}

fn visit_stmt(stmt: &Stmt, out: &mut HashSet<String>) {
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                visit_expr(expr, out);
            }
        }
        Stmt::Expr(expr) | Stmt::Throw(expr) | Stmt::Return(Some(expr)) => visit_expr(expr, out),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
        Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            visit_expr(condition, out);
            visit_stmts(then_branch, out);
            if let Some(else_branch) = else_branch {
                visit_stmts(else_branch, out);
            }
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            visit_expr(condition, out);
            visit_stmts(body, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init) = init {
                visit_stmt(init, out);
            }
            for expr in condition.iter().chain(update) {
                visit_expr(expr, out);
            }
            visit_stmts(body, out);
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            visit_expr(discriminant, out);
            for case in cases {
                if let Some(test) = &case.test {
                    visit_expr(test, out);
                }
                visit_stmts(&case.body, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            visit_stmts(body, out);
            if let Some(catch) = catch {
                visit_stmts(&catch.body, out);
            }
            if let Some(finally) = finally {
                visit_stmts(finally, out);
            }
        }
        Stmt::Labeled { body, .. } => visit_stmt(body, out),
        Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::ReleaseBoxes(_) => {}
    }
}
