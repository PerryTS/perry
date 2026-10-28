//! Hoisting of ordinary static ESM import bindings — extracted from
//! `module_decl.rs`, which had crossed the 2000-line size gate.

use super::*;
use swc_ecma_ast as ast;

/// Initialize a Node named import's snapshot-backed ESM export cell before
/// top-level user code can mutate the CommonJS namespace. This also preserves
/// the original value when the binding's first read happens after a mutation.
pub(super) fn init_named_cell(
    module: &mut Module,
    source: &str,
    imported: &str,
    method: Option<&String>,
) {
    if (method.is_none() && imported == "default")
        || !perry_api_manifest::is_node_core_module(source)
    {
        return;
    }
    module.init.insert(
        0,
        Stmt::Expr(Expr::Call {
            byte_end: 0,
            callee: Box::new(Expr::ExternFuncRef {
                name: "__perry_init_named_builtin_cell".to_string(),
                param_types: vec![Type::String, Type::String],
                return_type: Type::Any,
            }),
            args: vec![
                Expr::String(source.to_string()),
                Expr::String(imported.to_string()),
            ],
            type_args: vec![],
            byte_offset: 0,
        }),
    );
}

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
        if !is_bare_prefix_only_builtin(&raw_source)
            && (is_native_module_specifier(&raw_source)
                || is_node_builtin_module(&source)
                || source == "reflect-metadata")
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

/// Drop declaration-only export reads when lowering found no value use. Direct
/// native calls already name their implementation. Retain preloads for escapes,
/// reexports and synchronized calls so later CommonJS writes cannot change the
/// initial ESM binding.
pub(crate) fn prune_unused_named_cells(module: &mut Module) {
    fn key(expr: &Expr) -> Option<(String, String)> {
        let Expr::Call { callee, args, .. } = expr else {
            return None;
        };
        let Expr::ExternFuncRef { name, .. } = callee.as_ref() else {
            return None;
        };
        if name != "js_native_module_named_esm_export_value" {
            return None;
        }
        let [Expr::String(source), Expr::String(export)] = args.as_slice() else {
            return None;
        };
        Some((source.clone(), export.clone()))
    }
    let mut uses = std::collections::HashMap::<_, usize>::new();
    crate::for_each_module_expr(module, &mut |expr| {
        if let Some(key) = key(expr) {
            *uses.entry(key).or_default() += 1;
        }
    });
    // Native heritage stores the constructor value in class metadata instead
    // of an expression. Its prototype must exist before class registration.
    for class in &module.classes {
        if let Some(parent) = &class.native_extends {
            *uses.entry(parent.clone()).or_default() += 1;
        }
    }
    let exported: std::collections::HashSet<_> = module
        .exports
        .iter()
        .filter_map(|e| {
            if let Export::Named { local, .. } = e {
                Some(local.as_str())
            } else {
                None
            }
        })
        .collect();
    let reexports: std::collections::HashSet<_> = module
        .imports
        .iter()
        .flat_map(|i| {
            i.specifiers.iter().filter_map(|s| match s {
                ImportSpecifier::Named { imported, local } if exported.contains(local.as_str()) => {
                    Some((i.source.clone(), imported.clone()))
                }
                _ => None,
            })
        })
        .collect();
    module.init.retain_mut(|stmt| {
        let Stmt::Expr(expr) = stmt else { return true };
        let Expr::Call { callee, args, .. } = expr else {
            return true;
        };
        let Expr::ExternFuncRef { name, .. } = callee.as_mut() else {
            return true;
        };
        if name != "__perry_init_named_builtin_cell" {
            return true;
        }
        let [Expr::String(source), Expr::String(export)] = args.as_slice() else {
            unreachable!()
        };
        let key = (source.clone(), export.clone());
        *name = "js_native_module_named_esm_export_value".to_string();
        uses.contains_key(&key) || reexports.contains(&key)
    });
}
