//! Early error for a module binding declared twice through an import.
//!
//! ModuleItemList static semantics: the lexically declared names (imports,
//! top-level `function`/`class`/`let`/`const`) must be unique, and none of
//! them may also be a var-declared name. An import binding that a local
//! declaration repeats is `SyntaxError: Identifier 'x' has already been
//! declared` before any code runs. Without the check the local declaration
//! silently replaced the import.
//!
//! Only declarations that bind at run time count: type-only imports, function
//! overload signatures and `declare` forms bind nothing.
//!
//! In a TypeScript module a plain `import { Symbol }` binds nothing either when
//! the export it names is only a type (an interface or a type alias): tsc
//! elides the specifier, and a local `function Symbol` merges with it
//! (TypeScript's own `utilities.ts` does this). Whether the export is a type,
//! the exporting module states, and lowering one module cannot see it. So a
//! TypeScript named or default import that a local declaration repeats is
//! returned to the caller, which records it on the HIR module, and the compile
//! driver decides it once the import resolves
//! (`Module::locally_redeclared_imports`). A namespace import always binds the
//! module namespace object, and a JavaScript module elides nothing, so those
//! conflicts are rejected here.

use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

/// Checks the module's import bindings against its top-level declarations.
/// Returns the local names of TypeScript named/default imports a declaration
/// repeats: each is the SyntaxError only if its export has a value meaning.
pub(crate) fn check_import_redeclarations(
    ast_module: &ast::Module,
    source_file_path: &str,
) -> anyhow::Result<Vec<String>> {
    let elides_type_imports = crate::is_typescript_source(std::path::Path::new(source_file_path));
    // Local name -> whether only the resolved export can tell if it binds.
    let mut imported: HashMap<String, bool> = HashMap::new();
    for item in &ast_module.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import_decl)) = item else {
            continue;
        };
        if import_decl.type_only {
            continue;
        }
        for specifier in &import_decl.specifiers {
            let (local, may_name_a_type) = match specifier {
                ast::ImportSpecifier::Named(named) if named.is_type_only => continue,
                ast::ImportSpecifier::Named(named) => (&named.local, elides_type_imports),
                ast::ImportSpecifier::Default(default) => (&default.local, elides_type_imports),
                ast::ImportSpecifier::Namespace(namespace) => (&namespace.local, false),
            };
            let name = local.sym.to_string();
            if imported.insert(name.clone(), may_name_a_type).is_some() {
                return Err(already_declared(&name));
            }
        }
    }
    if imported.is_empty() {
        return Ok(Vec::new());
    }
    let mut declared = Vec::new();
    for item in &ast_module.body {
        match item {
            ast::ModuleItem::Stmt(ast::Stmt::Decl(decl)) => decl_names(decl, &mut declared),
            ast::ModuleItem::Stmt(stmt) => {
                crate::lower_decl::collect_var_binding_names_from_stmt(stmt, &mut declared)
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDecl(export)) => {
                decl_names(&export.decl, &mut declared)
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportDefaultDecl(export)) => {
                match &export.decl {
                    ast::DefaultDecl::Fn(f) if f.function.body.is_some() => {
                        declared.extend(f.ident.as_ref().map(|id| id.sym.to_string()))
                    }
                    ast::DefaultDecl::Class(c) => {
                        declared.extend(c.ident.as_ref().map(|id| id.sym.to_string()))
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let mut deferred = Vec::new();
    let mut seen = HashSet::new();
    for name in &declared {
        match imported.get(name.as_str()) {
            Some(false) => return Err(already_declared(name)),
            Some(true) if seen.insert(name.as_str()) => deferred.push(name.clone()),
            _ => {}
        }
    }
    Ok(deferred)
}

fn decl_names(decl: &ast::Decl, out: &mut Vec<String>) {
    match decl {
        ast::Decl::Fn(f) if !f.declare && f.function.body.is_some() => {
            out.push(f.ident.sym.to_string())
        }
        ast::Decl::Class(c) if !c.declare => out.push(c.ident.sym.to_string()),
        ast::Decl::Var(var) => {
            for declarator in &var.decls {
                if !crate::lower::ambient::declarator_binds_nothing(var, declarator) {
                    crate::lower_decl::collect_var_binding_names_from_pat(&declarator.name, out);
                }
            }
        }
        ast::Decl::TsEnum(e) if !e.declare => out.push(e.id.sym.to_string()),
        _ => {}
    }
}

fn already_declared(name: &str) -> anyhow::Error {
    anyhow::anyhow!("SyntaxError: Identifier '{name}' has already been declared")
}
