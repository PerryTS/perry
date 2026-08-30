//! Hoisting of ordinary static ESM import bindings — extracted from
//! `module_decl.rs`, which had crossed the 2000-line size gate.

use super::*;
use swc_ecma_ast as ast;

/// Register ordinary source-module import bindings before statement lowering.
///
/// ESM imports are module-scoped and hoisted regardless of where their
/// declarations appear in the source. The main declaration pass still emits
/// the HIR `Import` records in source order; this pre-pass only makes the
/// bindings visible to expressions that precede the declaration.
pub(crate) fn pre_register_static_import_bindings(
    ctx: &mut LoweringContext,
    ast_module: &ast::Module,
) {
    for item in &ast_module.body {
        let ast::ModuleItem::ModuleDecl(ast::ModuleDecl::Import(import_decl)) = item else {
            continue;
        };
        let raw_source = import_decl.src.value.as_str().unwrap_or("").to_string();
        let source = canonicalize_native_import_source(&raw_source);

        // Native imports need their module/method-specific registration, which
        // the ordinary declaration pass performs. This pass fixes source
        // modules, whose value bindings all share the imported-function path.
        if is_native_module(&source)
            || is_node_builtin_module(&source)
            || source == "reflect-metadata"
        {
            continue;
        }

        for specifier in &import_decl.specifiers {
            match specifier {
                ast::ImportSpecifier::Named(named) => {
                    if import_decl.type_only || named.is_type_only {
                        continue;
                    }
                    let local = named.local.sym.to_string();
                    ctx.register_imported_func(local.clone(), local);
                }
                ast::ImportSpecifier::Default(default) => {
                    if import_decl.type_only {
                        continue;
                    }
                    let local = default.local.sym.to_string();
                    ctx.register_imported_func(local.clone(), local.clone());
                    if source == "react" {
                        ctx.react_default_import_local = Some(local);
                    }
                }
                ast::ImportSpecifier::Namespace(namespace) => {
                    if import_decl.type_only {
                        continue;
                    }
                    let local = namespace.local.sym.to_string();
                    ctx.register_imported_func(local.clone(), local.clone());
                    ctx.namespace_import_locals.insert(local.clone());
                    ctx.namespace_import_sources
                        .insert(local.clone(), source.clone());
                    if source == "react" {
                        ctx.react_default_import_local = Some(local);
                    }
                }
            }
        }
    }
}

/// True when a syntactically value-shaped import declares ONLY per-specifier
/// type bindings.
///
/// TypeScript's
///
///   import { type Foo, type Bar } from "./types"
///
/// is just as runtime-erased as `import type { Foo, Bar }`. Callers keep the
/// named specifiers so class/interface metadata still reaches consumers, but
/// the declaration itself creates no runtime binding and no module-init edge.
/// A mixed declaration retains its runtime edge because at least one specifier
/// carries a value binding.
pub(crate) fn import_is_runtime_erased(
    import_decl: &ast::ImportDecl,
    whole_decl_type_only: bool,
) -> bool {
    !whole_decl_type_only
        && !import_decl.specifiers.is_empty()
        && import_decl.specifiers.iter().all(|specifier| {
            matches!(
                specifier,
                ast::ImportSpecifier::Named(named) if named.is_type_only
            )
        })
}
