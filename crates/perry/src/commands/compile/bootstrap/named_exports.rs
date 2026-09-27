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

/// A user module's static named/default import of a bare package that did
/// not resolve, whose binding the module reads as a value. Perry has nothing to
/// bind it to: codegen emitted an extern reference named after the LOCAL
/// binding, and the program failed in the linker on `_nanoid` / `_uuidv4` /
/// `_fastify` with no mention of the missing package (#11576). Node fails the
/// same program at load time with ERR_MODULE_NOT_FOUND, so a compile error is
/// the faithful answer. Deliberately narrow:
///
/// * only bare package specifiers — relative paths, `node:`/builtin names
///   (runtime-backed or not), and Perry's own `perry/*` modules keep today's
///   behaviour;
/// * only importers outside `node_modules`, and never CJS-wrapped
///   `require()`s: packages routinely `require` optional peers inside a
///   `try` (`debug` → `supports-color`), which must stay non-fatal;
/// * only bindings the module CALLS directly (see [`called_import_names`]) —
///   exactly the shape that used to die in the linker. A property read or a
///   `new` on an unresolved binding still warns and continues as before (it
///   links, and throws where it is reached), and a type-only use of an
///   uninstalled `@types`-style import still compiles.
fn enforce_resolved_bare_imports(ctx: &CompilationContext) -> Result<()> {
    let mut missing = Vec::new();
    for (importer, module) in &ctx.native_modules {
        if importer
            .components()
            .any(|component| component.as_os_str() == "node_modules")
        {
            continue;
        }
        let mut called: Option<HashSet<String>> = None;
        for import in &module.imports {
            if import.resolved_path.is_some()
                || import.is_native
                || import.type_only
                || import.runtime_erased
                || import.is_dynamic
                || import.is_adopted_require
                || !is_bare_package_specifier(&import.source)
            {
                continue;
            }
            for specifier in &import.specifiers {
                let local = match specifier {
                    ImportSpecifier::Named { local, .. } | ImportSpecifier::Default { local } => {
                        local
                    }
                    // Namespace imports already hard-error at collection (#629).
                    ImportSpecifier::Namespace { .. } => continue,
                };
                if called
                    .get_or_insert_with(|| called_import_names(module))
                    .contains(local)
                {
                    missing.push((importer.clone(), import.source.clone(), local.clone()));
                }
            }
        }
    }
    missing.sort();
    missing.dedup();
    if let Some((importer, source, _)) = missing.first().cloned() {
        let package = package_name(&source);
        // Every binding from the SAME package in the same importer, so the
        // message names all of them (`uuidv4` and `nanoid` in one snippet).
        let locals: Vec<String> = missing
            .iter()
            .filter(|(i, s, _)| *i == importer && package_name(s) == package)
            .map(|(_, _, local)| format!("'{local}'"))
            .collect();
        let others: Vec<&str> = {
            let mut v: Vec<&str> = missing
                .iter()
                .map(|(_, s, _)| package_name(s))
                .filter(|p| *p != package)
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let also = if others.is_empty() {
            String::new()
        } else {
            format!("\nAlso not installed: {}.", others.join(", "))
        };
        bail!(
            "Cannot find package '{package}' imported from {} (binding {} of `import ... from \"{source}\"`).\n\
             Perry compiles npm packages from their installed source, and '{package}' is not installed \
             in any node_modules directory above the importer. Install it (e.g. `npm install {package}`) \
             and compile again.{also}",
            importer.display(),
            locals.join(", ")
        );
    }
    Ok(())
}

/// `lodash` / `@scope/pkg` / `pkg/sub/path`, but not `./x`, `/abs`, `node:fs`,
/// a Node builtin name, `perry/ui`, or a URL-like specifier.
fn is_bare_package_specifier(source: &str) -> bool {
    if source.is_empty()
        || source.starts_with('.')
        || source.starts_with('/')
        || source.starts_with('#')
        || source.contains(':')
        || source == "perry"
        || source.starts_with("perry/")
    {
        return false;
    }
    let name = package_name(source);
    !perry_hir::is_node_builtin_module(name) && !perry_hir::is_node_builtin_module(source)
}

/// The package part of a bare specifier: `@scope/pkg/sub` → `@scope/pkg`,
/// `pkg/sub` → `pkg`.
fn package_name(source: &str) -> &str {
    let mut parts = source.splitn(3, '/');
    let first = parts.next().unwrap_or(source);
    if first.starts_with('@') {
        match parts.next() {
            Some(second) => &source[..first.len() + 1 + second.len()],
            None => source,
        }
    } else {
        first
    }
}

/// Reject statically absent named exports while module/source context is still
/// available, instead of inventing a perry_fn symbol that fails in the linker.
pub(super) fn enforce(ctx: &mut CompilationContext) -> Result<()> {
    enforce_resolved_bare_imports(ctx)?;
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
    collect_refs(module).values
}

/// Imported bindings `module` CALLS directly (`nanoid()`, `uuidv4()`): the
/// shape that codegen lowers to a call of an extern symbol named after the
/// local binding, so an unresolved one fails in the linker (#11576).
fn called_import_names(module: &Module) -> HashSet<String> {
    collect_refs(module).called
}

#[derive(Default)]
struct Refs {
    values: HashSet<String>,
    called: HashSet<String>,
}

impl Refs {
    fn insert(&mut self, name: String) {
        self.values.insert(name);
    }
}

fn collect_refs(module: &Module) -> Refs {
    let mut out = Refs::default();
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

fn visit_class(class: &Class, out: &mut Refs) {
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

fn visit_decorators(decorators: &[Decorator], out: &mut Refs) {
    for decorator in decorators {
        out.insert(decorator.name.clone());
        for arg in &decorator.args {
            visit_expr(arg, out);
        }
    }
}

fn visit_function(function: &Function, out: &mut Refs) {
    visit_decorators(&function.decorators, out);
    for param in &function.params {
        visit_decorators(&param.decorators, out);
        if let Some(default) = &param.default {
            visit_expr(default, out);
        }
    }
    visit_stmts(&function.body, out);
}

fn visit_expr(expr: &Expr, out: &mut Refs) {
    if let Expr::Call { callee, .. } = expr {
        if let Expr::ExternFuncRef { name, .. } = callee.as_ref() {
            out.called.insert(name.clone());
        }
    }
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

fn visit_stmts(stmts: &[Stmt], out: &mut Refs) {
    for stmt in stmts {
        visit_stmt(stmt, out);
    }
}

fn visit_stmt(stmt: &Stmt, out: &mut Refs) {
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

#[cfg(test)]
mod unresolved_import_tests {
    use super::{is_bare_package_specifier, package_name};

    #[test]
    fn bare_specifiers_are_the_packages_node_would_look_up() {
        for bare in [
            "nanoid",
            "uuid",
            "@scope/pkg",
            "@scope/pkg/sub",
            "mysql2/promise",
        ] {
            assert!(is_bare_package_specifier(bare), "{bare}");
        }
        for not_bare in [
            "./x",
            "../y",
            "/abs/z",
            "#internal",
            "node:fs",
            "fs",
            "fs/promises",
            "perry/ui",
            "perry",
            "https://x.dev/m.js",
            "",
        ] {
            assert!(!is_bare_package_specifier(not_bare), "{not_bare}");
        }
    }

    #[test]
    fn package_name_strips_subpaths_and_keeps_scopes() {
        assert_eq!(package_name("nanoid"), "nanoid");
        assert_eq!(package_name("mysql2/promise"), "mysql2");
        assert_eq!(package_name("@scope/pkg"), "@scope/pkg");
        assert_eq!(package_name("@scope/pkg/deep/path"), "@scope/pkg");
    }
}
