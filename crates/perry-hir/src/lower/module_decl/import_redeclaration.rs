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

use std::collections::HashSet;
use swc_ecma_ast as ast;

pub(crate) fn check_import_redeclarations(ast_module: &ast::Module) -> anyhow::Result<()> {
    let mut imported: HashSet<String> = HashSet::new();
    for item in &ast_module.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import_decl)) = item else {
            continue;
        };
        if import_decl.type_only {
            continue;
        }
        for specifier in &import_decl.specifiers {
            let local = match specifier {
                ast::ImportSpecifier::Named(named) if named.is_type_only => continue,
                ast::ImportSpecifier::Named(named) => &named.local,
                ast::ImportSpecifier::Default(default) => &default.local,
                ast::ImportSpecifier::Namespace(namespace) => &namespace.local,
            };
            let name = local.sym.to_string();
            if !imported.insert(name.clone()) {
                return Err(already_declared(&name));
            }
        }
    }
    if imported.is_empty() {
        return Ok(());
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
    match declared.iter().find(|name| imported.contains(name.as_str())) {
        Some(name) => Err(already_declared(name)),
        None => Ok(()),
    }
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
