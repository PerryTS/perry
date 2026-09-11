//! Source-free subset selection shares auto-mode's required-feature analysis.
//! A missing archive or any unsupported requirement retains the full fallback.

use super::{auto_optimized_cross_features, CompilationContext};
use std::collections::BTreeSet;

// The existing conservative member-name analysis also sees `console.log` as
// a possible `Math.log` value read. Keep Math's namespace in the small profile
// rather than weakening that analysis and risking a missing extracted method.
const CORE_FEATURES: &[&str] = &[
    "full",
    "alloc-mimalloc",
    "keepalive-anchors",
    "global-math",
];

pub(super) fn eligible(ctx: &CompilationContext, cli_features: &[String]) -> bool {
    // The first packaged subset supports runtime-only native programs. Keep
    // native modules, dynamic/native callbacks and custom feature requests on
    // their existing archive paths, even if today's feature list looks small.
    if ctx.needs_stdlib
        || ctx.needs_ui
        || ctx.needs_thread
        || ctx.needs_plugins
        || ctx.needs_geisterhand
        || ctx.needs_wasm_runtime
        || !ctx.native_addons.is_empty()
        || !ctx.native_module_imports.is_empty()
        || !ctx.extra_stdlib_features.is_empty()
        || !cli_features.is_empty()
        || ctx.native_modules.values().any(needs_global_object_fallback)
        // Runtime-owned native modules such as bun:ffi do not enter the
        // stdlib feature/import set above. Retain their original HIR import
        // provenance so they cannot accidentally select the first subset.
        || ctx.native_modules.values().any(|module| {
            module.imports.iter().any(|import| {
                import.is_native && !import.type_only && !import.runtime_erased
            })
        })
    {
        return false;
    }
    auto_optimized_cross_features(ctx, &BTreeSet::new(), cli_features)
        .iter()
        .all(|feature| {
            feature
                .strip_prefix("perry-runtime/")
                .is_some_and(|name| CORE_FEATURES.contains(&name))
        })
}

fn needs_global_object_fallback(module: &perry_hir::Module) -> bool {
    if module.references_global_this {
        return true;
    }
    // Runtime keys can select any optional constructor/namespace without its
    // name appearing in the source: globalThis[process.argv[2]]. Include HIR
    // spellings as well as the source flag to cover global/self aliases,
    // escaped identifier spellings and folded `Function("return this")()`.
    // As in the existing feature detector, over-inclusion only costs size.
    let hir = format!("{module:?}");
    [
        "GlobalThisExpr",
        "property: \"globalThis\"",
        "property: \"global\"",
        "property: \"self\"",
    ]
    .iter()
    .any(|token| hir.contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> CompilationContext {
        CompilationContext::new(std::env::current_dir().unwrap())
    }

    #[test]
    fn core_profile_uses_the_same_feature_contract_as_packaging() {
        let root = super::super::super::find_perry_workspace_root().unwrap();
        let manifest: toml::Value = toml::from_str(
            &std::fs::read_to_string(root.join("crates/perry-runtime/Cargo.toml")).unwrap(),
        )
        .unwrap();
        let packaged: BTreeSet<_> = manifest["features"]["prebuilt-core"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(packaged, CORE_FEATURES.iter().copied().collect());
        assert!(eligible(&context(), &[]));
        let mut console_or_math = context();
        console_or_math.uses_global_math = true;
        assert!(eligible(&console_or_math, &[]));
    }

    #[test]
    fn optional_engines_and_dynamic_code_keep_the_full_fallback() {
        for configure in [
            |c: &mut CompilationContext| c.uses_regex = true,
            |c: &mut CompilationContext| c.uses_temporal = true,
            |c: &mut CompilationContext| c.uses_url = true,
            |c: &mut CompilationContext| c.uses_string_normalize = true,
            |c: &mut CompilationContext| c.uses_intl_segmenter = true,
            |c: &mut CompilationContext| c.uses_intl_namespace = true,
            |c: &mut CompilationContext| c.uses_intl_locale = true,
            |c: &mut CompilationContext| c.uses_intl_datetime = true,
            |c: &mut CompilationContext| c.uses_data_url_dynamic_import = true,
            |c: &mut CompilationContext| c.uses_diagnostics = true,
            |c: &mut CompilationContext| c.uses_global_json = true,
            |c: &mut CompilationContext| c.uses_global_reflect = true,
            |c: &mut CompilationContext| c.uses_global_atomics = true,
            |c: &mut CompilationContext| c.uses_global_url = true,
            |c: &mut CompilationContext| c.uses_global_text = true,
            |c: &mut CompilationContext| c.uses_global_websocket = true,
            |c: &mut CompilationContext| c.uses_global_webcrypto = true,
            |c: &mut CompilationContext| c.uses_global_webfetch = true,
            |c: &mut CompilationContext| c.uses_proc_ipc = true,
            |c: &mut CompilationContext| c.bun_platform = true,
        ] {
            let mut ctx = context();
            configure(&mut ctx);
            assert!(!eligible(&ctx, &[]));
        }
    }

    #[test]
    fn native_modules_callbacks_and_custom_features_keep_existing_archives() {
        for configure in [
            |c: &mut CompilationContext| c.needs_stdlib = true,
            |c: &mut CompilationContext| c.needs_ui = true,
            |c: &mut CompilationContext| c.needs_thread = true,
            |c: &mut CompilationContext| c.needs_plugins = true,
            |c: &mut CompilationContext| c.needs_geisterhand = true,
            |c: &mut CompilationContext| c.needs_wasm_runtime = true,
            |c: &mut CompilationContext| {
                c.native_module_imports.insert("vm".into());
            },
            |c: &mut CompilationContext| {
                c.native_module_imports.insert("worker_threads".into());
            },
            |c: &mut CompilationContext| {
                c.native_module_imports.insert("bun:ffi".into());
            },
        ] {
            let mut ctx = context();
            configure(&mut ctx);
            assert!(!eligible(&ctx, &[]));
        }
        assert!(!eligible(&context(), &["ios-game-loop".into()]));
    }

    #[test]
    fn runtime_owned_native_imports_keep_full_without_a_stdlib_marker() {
        let mut ctx = context();
        let mut module = perry_hir::Module::new("ffi-consumer");
        module.imports.push(perry_hir::Import {
            source: "bun:ffi".into(),
            specifiers: Vec::new(),
            is_native: true,
            module_kind: perry_hir::ModuleKind::NativeRust,
            resolved_path: None,
            type_only: false,
            runtime_erased: false,
            is_dynamic: false,
            is_dynamic_target: false,
            is_deferred_require: false,
            is_adopted_require: false,
        });
        let key = std::path::PathBuf::from("ffi-consumer.ts");
        ctx.native_modules.insert(key.clone(), module);
        assert!(ctx.native_module_imports.is_empty());
        assert!(!ctx.needs_stdlib);
        assert!(!eligible(&ctx, &[]));
        ctx.native_modules.get_mut(&key).unwrap().imports[0].type_only = true;
        assert!(eligible(&ctx, &[]));
        let import = &mut ctx.native_modules.get_mut(&key).unwrap().imports[0];
        import.type_only = false;
        import.runtime_erased = true;
        assert!(eligible(&ctx, &[]));
    }

    #[test]
    fn global_object_values_keep_optional_engines_available() {
        let key = std::path::PathBuf::from("global-consumer.ts");
        for global in ["globalThis", "global", "self"] {
            let mut ctx = context();
            let mut module = perry_hir::Module::new("global-consumer");
            module.init.push(perry_hir::Stmt::Expr(perry_hir::Expr::PropertyGet {
                object: Box::new(perry_hir::Expr::GlobalGet(0)),
                property: global.into(),
                byte_offset: 0,
            }));
            ctx.native_modules.insert(key.clone(), module);
            assert!(!eligible(&ctx, &[]), "{global} can expose optional engines");
        }
        let mut ctx = context();
        let mut module = perry_hir::Module::new("global-consumer");
        module.init.push(perry_hir::Stmt::Expr(perry_hir::Expr::GlobalThisExpr));
        ctx.native_modules.insert(key.clone(), module);
        assert!(!eligible(&ctx, &[]));
        let module = ctx.native_modules.get_mut(&key).unwrap();
        module.init.clear();
        module.references_global_this = true;
        assert!(!eligible(&ctx, &[]));
    }
}
