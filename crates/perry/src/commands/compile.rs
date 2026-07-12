//! Compile command - compiles TypeScript to native executable

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::OutputFormat;

// Tier 2.1 (v0.5.333): split out self-contained sub-concerns into the
// `compile/` directory. The `compile.rs` orchestrator stays as the
// public API surface; helpers move to focused modules so unrelated
// changes don't churn this file.
mod app_metadata;
mod apple_codesign;
mod apple_info_plist;
mod audit_manifest;
mod bootstrap;
mod build_cache;
mod bundle_apple;
mod bundle_ios;
mod cjs_wrap;
mod codegen_steps;
mod collect_modules;
mod compressed_libs;
mod embed;
mod env_fold;
mod harmonyos_shim;
mod host_config;
mod i18n_emit;
mod init_order;
mod library_search;
mod link;
mod lock_scan;
mod lowering_report;
mod object_cache;
mod optimized_libs;
mod parse_cache;
mod post_link;
mod precompile_capture;
mod reachability;
mod resolve;
mod resources;
mod sandbox_buildrs;
mod strip_dedup;
mod targets;
pub mod well_known;
pub(crate) mod widget_build;
use app_metadata::rust_target_triple;
// apple_info_plist helpers used through bundle_ios (no direct uses in
// compile.rs anymore now that the iOS bundle code moved out).
pub(crate) use audit_manifest::allowlist_matches;
use bootstrap::{
    apply_i18n_pass, bundle_extensions_into_ctx, dump_hir_for_debug, maybe_init_type_checker,
    rerun_collect_with_class_field_types, run_native_instance_fixups, run_post_collect_preflight,
};
// HarmonyOS ArkTS harvest only exists when the arkts backend is compiled in.
#[cfg(feature = "backend-arkts")]
use bootstrap::harvest_harmonyos_index_ets;
use build_cache::BuildCacheProbe;
use bundle_apple::{bundle_for_tvos, bundle_for_visionos, bundle_for_watchos};
use bundle_ios::build_ios_app_bundle;
use collect_modules::collect_modules;
use harmonyos_shim::emit_harmonyos_arkts_stubs;
use host_config::apply_pkg_and_toml_config;
use i18n_emit::{emit_android_i18n_resources, write_i18n_key_registry};
use init_order::{classify_eager_modules, topo_sort_non_entry_modules};
pub use library_search::find_library;
pub(crate) use library_search::host_target_triple;
use library_search::{
    build_geisterhand_libs, find_geisterhand_library, find_geisterhand_runtime,
    find_geisterhand_stdlib, find_geisterhand_ui, find_harmonyos_sdk, find_lld_link,
    find_llvm_tool, find_msvc_lib_paths, find_msvc_link_exe, find_perry_windows_sdk,
    find_runtime_library, find_stdlib_library, find_ui_library, find_wasm_host_library,
    windows_default_output_extension, windows_pe_subsystem_flag, windows_subsystem_needs_ui,
};
use link::{build_and_run_link, write_link_cache_manifest};
pub use lock_scan::collect_native_archives_for_lock;
pub(crate) use lock_scan::run_lock_verify_for_compile;
pub use object_cache::ObjectCache;
pub use object_cache::{cache_dir_override, resolve_cache_dir};
use object_cache::{compute_object_cache_key, djb2_hash};
use optimized_libs::{build_optimized_libs, OptimizedLibs};
use parse_cache::parse_cached;
pub use parse_cache::ParseCache;
use post_link::{
    cleanup_intermediates, emit_attestation_sidecar, print_binary_size, strip_final_binary,
    summarize_codegen_cache_stats,
};
pub use resolve::find_perry_workspace_root;
pub(crate) use resolve::validate_native_library_manifest_value;
use resolve::{
    cached_resolve_import, compute_module_prefix, declaration_sidecar_for_resolved_import,
    ergonomic_export_alias, extract_compile_package_dir, has_perry_native_library,
    is_declaration_file, is_in_compile_package, is_in_perry_native_package, is_js_file,
    is_recognized_text_asset, parse_native_library_manifest, parse_package_specifier,
    resolve_import,
};
use strip_dedup::{
    dedup_native_lib_for_tier3, dedup_runtime_for_tier3, dedup_stdlib_for_tier3,
    dedup_ui_lib_against_linked_libs, localize_stdlib_stub_symbols,
    localize_stdlib_stub_symbols_for_windows, strip_bundled_runtime_from_well_known_lib,
    strip_bundled_shared_deps_from_well_known_lib, strip_duplicate_objects_from_lib,
    strip_duplicate_objects_from_well_known_lib,
};
use targets::{
    apple_sdk_version, find_visionos_swift_runtime, find_watchos_swift_runtime,
    generate_embedded_js_object, generate_js_bundle,
};
// Codegen-backend entrypoints (#5422) — only present when their backend feature
// is compiled in. The matching `--target` routing below errors cleanly when a
// backend was built out.
#[cfg(feature = "backend-glance")]
use targets::compile_for_android_widget;
#[cfg(feature = "backend-wasm")]
use targets::compile_for_wasm;
#[cfg(feature = "backend-wear-tiles")]
use targets::compile_for_wearos_tile;
#[cfg(feature = "backend-swiftui")]
use targets::{compile_for_ios_widget, compile_for_watchos_widget};

use super::progress::{ProgressSnapshot, VerboseProgress};

mod types;
pub use types::*;

// Tier (split-large-files): the small standalone helpers and the giant
// `run_with_parse_cache` orchestrator were relocated into sibling modules to
// keep this trunk small. They are re-exported here so existing call paths keep
// resolving.
mod helpers;
mod run_pipeline;

#[cfg(windows)]
pub(crate) use helpers::is_windows_reserved_file_stem;
pub(crate) use helpers::{
    apply_libc_to_target, backend_disabled_msg, canonical_class_source_prefix,
    native_object_file_stem, object_cache_project_root, print_deferred_eval_notice,
    NativeObjectArtifact,
};
pub use run_pipeline::run_with_parse_cache;

// `inject_ios_deeplinks`, `inject_google_auth_info_plist`, and
// `lookup_bundle_id_from_info_plist` moved to `apple_info_plist.rs`.
// `rust_target_triple` moved to `app_metadata.rs`.
// `emit_harmonyos_arkts_stubs` moved to `harmonyos_shim.rs`.

// Phase helpers (maybe_init_type_checker, bundle_extensions_into_ctx,
// rerun_collect_with_class_field_types, apply_geisterhand_args) moved
// to compile/bootstrap.rs alongside the newer post-collect preflight
// and native-instance fixup helpers.

pub fn run(
    args: CompileArgs,
    format: OutputFormat,
    use_color: bool,
    verbose: u8,
) -> Result<CompileResult> {
    run_with_parse_cache(args, None, format, use_color, verbose)
}

#[cfg(test)]
mod windows_link_tests;

// `app_metadata_tests` moved to `compile/app_metadata.rs`.

// `js_runtime_gate_tests` and `allowlist_tests` moved to
// `compile/audit_manifest.rs` alongside the helpers they cover.

// (allowlist_tests body removed — coverage lives in compile/audit_manifest.rs)

// `collect_native_archives_for_lock`, `for_each_native_library_package`,
// `derive_target_key`, `run_lock_verify_for_compile`, and the
// `lock_integration_tests` module all moved to `compile/lock_scan.rs`.

// (lock_integration_tests body removed — coverage lives in compile/lock_scan.rs)
