//! Published barrels (notably Remeda) spell re-exports as imports followed by
//! `export { local as public }`. Normalize only modules whose runtime imports
//! are bare imports or named bindings forwarded through local export lists,
//! with an entirely side-effect-free, acyclic static dependency tree. Bare
//! imports remain imports; the cycle proof permits moving forwarded bindings
//! after them without changing the initialization of exported values.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use swc_ecma_ast as ast;

use super::CompilationContext;

pub(crate) fn normalize(
    module: &ast::Module,
    path: &Path,
    ctx: &mut CompilationContext,
) -> Option<ast::Module> {
    if !super::enabled()
        || !module.body.iter().all(|item| {
            matches!(
                item,
                ast::ModuleItem::ModuleDecl(
                    ast::ModuleDecl::Import(_)
                        | ast::ModuleDecl::ExportNamed(_)
                        | ast::ModuleDecl::ExportAll(_)
                ) | ast::ModuleItem::Stmt(ast::Stmt::Empty(_))
            )
        })
    {
        return None;
    }
    // Import aliases/attributes can change resolution or select a file loader.
    // Re-export collection does not share those paths; retain such imports.
    if module.body.iter().any(|item| {
        matches!(item,
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import))
                if import.phase != ast::ImportPhase::Evaluation
                    || import.with.is_some()
                    || ctx.package_aliases.contains_key(import.src.value.to_string_lossy().as_ref())
        )
    }) {
        return None;
    }

    let mut exports: HashMap<String, Vec<ast::NamedExport>> = HashMap::new();
    for item in &module.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportNamed(export)) = item else {
            continue;
        };
        if export.src.is_some() || export.type_only {
            continue;
        }
        for spec in &export.specifiers {
            let ast::ExportSpecifier::Named(named) = spec else {
                return None;
            };
            if named.is_type_only {
                continue;
            }
            let ast::ModuleExportName::Ident(local) = &named.orig else {
                return None;
            };
            let mut single = export.clone();
            let mut named = named.clone();
            // Renaming orig to the imported name below must not rename the
            // public export when the original list omitted `as public`.
            named.exported = Some(named.exported.clone().unwrap_or_else(|| named.orig.clone()));
            single.specifiers = vec![ast::ExportSpecifier::Named(named)];
            exports
                .entry(local.sym.to_string())
                .or_default()
                .push(single);
        }
    }
    let candidates: HashSet<String> = module
        .body
        .iter()
        .filter_map(|item| match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) if !import.type_only => {
                Some(import)
            }
            _ => None,
        })
        .flat_map(|import| import.specifiers.iter())
        .filter_map(|spec| match spec {
            ast::ImportSpecifier::Named(named)
                if !named.is_type_only && exports.contains_key(named.local.sym.as_ref()) =>
            {
                Some(named.local.sym.to_string())
            }
            _ => None,
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }

    // Keep namespaces and bindings with uses other than forwarding. Genuine
    // bare imports can stay in place: can_drop_tree below proves the complete
    // graph free of effects AND cycles before any dependency order changes.
    // This covers generated barrels that repeat their shared helper chunks
    // as bare imports, as well as mixed direct/indirect re-exports.
    if module.body.iter().any(|item| match item {
        ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) if !import.type_only => {
            import.specifiers.iter().any(|spec| match spec {
                ast::ImportSpecifier::Named(named) => {
                    !named.is_type_only && !candidates.contains(named.local.sym.as_ref())
                }
                _ => true,
            })
        }
        _ => false,
    }) {
        return None;
    }

    let mut state = std::mem::take(&mut ctx.reexport_pruner);
    let safe = state.scan.can_drop_tree(path, ctx);
    ctx.reexport_pruner = state;
    if !safe {
        return None;
    }

    let mut result = module.clone();
    result.body.clear();
    for item in &module.body {
        match item {
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import)) if !import.type_only => {
                let mut remaining = import.clone();
                let mut forwarded = Vec::new();
                remaining.specifiers.retain(|spec| {
                    let ast::ImportSpecifier::Named(named) = spec else {
                        return true;
                    };
                    if named.is_type_only || !candidates.contains(named.local.sym.as_ref()) {
                        return true;
                    }
                    let imported = named
                        .imported
                        .clone()
                        .unwrap_or_else(|| ast::ModuleExportName::Ident(named.local.clone()));
                    for template in &exports[named.local.sym.as_ref()] {
                        let mut export = template.clone();
                        export.span = import.span;
                        export.src = Some(import.src.clone());
                        export.with = import.with.clone();
                        let ast::ExportSpecifier::Named(spec) = &mut export.specifiers[0] else {
                            unreachable!();
                        };
                        spec.orig = imported.clone();
                        forwarded.push(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportNamed(
                            export,
                        )));
                    }
                    false
                });
                // Preserve genuine bare imports, even under sideEffects:false.
                if !remaining.specifiers.is_empty() || forwarded.is_empty() {
                    result
                        .body
                        .push(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(
                            remaining,
                        )));
                }
                result.body.extend(forwarded);
            }
            ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportNamed(export))
                if export.src.is_none() && !export.type_only =>
            {
                let mut remaining = export.clone();
                remaining.specifiers.retain(|spec| match spec {
                    ast::ExportSpecifier::Named(named) if !named.is_type_only => {
                        match &named.orig {
                            ast::ModuleExportName::Ident(local) => {
                                !candidates.contains(local.sym.as_ref())
                            }
                            _ => true,
                        }
                    }
                    _ => true,
                });
                if !remaining.specifiers.is_empty() {
                    result
                        .body
                        .push(ast::ModuleItem::ModuleDecl(ast::ModuleDecl::ExportNamed(
                            remaining,
                        )));
                }
            }
            _ => result.body.push(item.clone()),
        }
    }
    Some(result)
}
