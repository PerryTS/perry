//! Source-free subset selection shares auto-mode's required-feature analysis.
//! A missing archive or any unsupported requirement retains the full fallback.

use super::{auto_optimized_cross_features, CompilationContext};
use std::collections::BTreeSet;

const CORE_FEATURES: &[&str] = &["full", "alloc-mimalloc", "keepalive-anchors"];

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
            |c: &mut CompilationContext| c.uses_global_math = true,
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
}
