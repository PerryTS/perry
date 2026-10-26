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
    if method.is_none() || !perry_api_manifest::is_node_core_module(source) {
        return;
    }
    module.init.insert(
        0,
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_native_module_named_esm_export_value".to_string(),
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

/// Register one native named binding and its runtime enum metadata. FS promises
/// uses the ordinary parent export cell: its value is the CommonJS object,
/// not the distinct fs/promises ESM namespace.
pub(super) fn register_native_named_import(
    ctx: &mut LoweringContext,
    module: &mut Module,
    source: &str,
    raw_source: &str,
    imported: &str,
    local: &str,
    span: swc_common::Span,
) -> bool {
    let is_node_core = perry_api_manifest::is_node_core_module(source);
    if typescript::should_defer_unknown_named_import(source, imported, is_node_core) {
        // Almost always a TS TYPE in a mixed import —
        // `import { createCipheriv, BinaryLike } from "crypto"`.
        // tsc/esbuild/Bun all erase such a specifier; bailing here
        // failed the whole file. Defer: record it, bind nothing, and
        // let `lower_ident_expr` raise this same error if the local is
        // ever referenced as a VALUE (type positions are erased before
        // expression lowering, so a genuine bad import still errors).
        ctx.deferred_unknown_native_imports.insert(
            local.to_string(),
            (imported.to_string(), raw_source.to_string(), span),
        );
        return false;
    }
    // Register as native module function with the original method name
    // e.g., import { v4 as uuid } from 'uuid' -> uuid maps to uuid.v4.
    //
    // Some named exports are object-valued submodule namespaces rather
    // than callables. Route those locals to the canonical submodule so
    // `import { types } from "node:util"; types.isX()` uses the same
    // dispatch as `node:util/types` and `util.types.isX()`.
    let (native_module, native_method) = if is_node_core && imported == "default" {
        (source.to_string(), None)
    } else if source == "util" && imported == "types" {
        ("util.types".to_string(), None)
    } else if source == "punycode" && imported == "ucs2" {
        ("punycode.ucs2".to_string(), None)
    } else if source == "inspector"
        && matches!(imported, "Network" | "NetworkResources" | "DOMStorage")
    {
        (format!("inspector.{imported}"), None)
    } else if matches!(source, "dns" | "stream" | "readline" | "timers") && imported == "promises" {
        // Preserve the existing member-call routes of the other promise modules.
        (format!("{source}/promises"), None)
    } else {
        (source.to_string(), Some(imported.to_string()))
    };
    init_named_cell(module, source, imported, native_method.as_ref());
    ctx.register_native_module(local.to_string(), native_module, native_method);
    // #1991: `perry/ui` exposes these as numeric
    // `const enum`s in `types/perry/ui/index.d.ts`.
    // Native modules do not have source HIR enum
    // declarations, so register the imported binding as
    // a real HIR enum at the import site. That lets the
    // existing enum-member lowering/codegen path handle
    // `Key.Space`, including aliases like
    // `import { Key as K } from "perry/ui"; K.Space`,
    // instead of treating it as a runtime property read
    // from a non-existent native-module object.
    if source == "perry/ui" {
        if let Some(model_members) = perry_ui_model::const_enum_members(imported) {
            let enum_id = ctx.fresh_enum();
            let members: Vec<EnumMember> = model_members
                .into_iter()
                .map(|member| EnumMember {
                    name: member.name.to_string(),
                    value: EnumValue::Number(member.value as f64),
                })
                .collect();
            let member_values = members
                .iter()
                .map(|m| (m.name.clone(), m.value.clone()))
                .collect();
            ctx.define_enum(local.to_string(), enum_id, member_values);
            module.enums.push(Enum {
                id: enum_id,
                name: local.to_string(),
                members,
                is_exported: false,
            });
        }
    }
    typescript::register_runtime_enum(ctx, module, source, local, imported);
    true
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
