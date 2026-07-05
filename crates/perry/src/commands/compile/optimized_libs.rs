//! Auto-rebuild perry-runtime + perry-stdlib with the smallest matching
//! Cargo feature set so the compiled `.o` only links the runtime APIs
//! the user's TS code actually uses.
//!
//! Tier 2.1 follow-up (v0.5.341) — extracts `OptimizedLibs` + the
//! `build_optimized_libs` driver from `compile.rs`. ~390 LOC of
//! self-contained library-build orchestration. Both `runtime` and
//! `stdlib` halves fall back to the prebuilt libraries gracefully on
//! any failure (no source on disk, no cargo, build error). Cargo's
//! incremental cache is keyed per (target dir, feature set), and we
//! use a hash-keyed target dir so consecutive runs with the same
//! profile are no-ops after the first build.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::library_search::{find_harmonyos_sdk, harmonyos_cross_env};
use super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

mod driver;
mod freshness;
mod no_auto;
mod paths;

pub(crate) use driver::build_optimized_libs;
pub(crate) use freshness::{
    auto_optimized_archives_are_fresh, auto_optimized_build_stamp, auto_optimized_cache_key,
    auto_optimized_cross_features, auto_optimized_source_fingerprint, binding_needs_shared_tokio,
    resolve_auto_well_known_libs,
};
pub(crate) use no_auto::{
    build_missing_prebuilt_ext_lib, resolve_no_auto_optimized_libs, resolve_prebuilt_ext_libs,
};
pub(crate) use paths::{
    android_global_dynamic_tls_rustflag, auto_target_dir_paths, cargo_target_dir_path,
};

#[cfg(test)]
mod tests;

pub struct OptimizedLibs {
    /// Path to the rebuilt `libperry_runtime.a` (or `perry_runtime.lib`).
    /// `None` means "fall back to the prebuilt one in target/release/".
    pub runtime: Option<PathBuf>,
    /// Path to the rebuilt `libperry_stdlib.a`. `None` means "fall back
    /// to the prebuilt full stdlib".
    pub stdlib: Option<PathBuf>,
    /// LLVM bitcode (`.bc`) for perry-runtime (Phase J).
    pub runtime_bc: Option<PathBuf>,
    /// LLVM bitcode (`.bc`) for perry-stdlib (Phase J).
    pub stdlib_bc: Option<PathBuf>,
    /// LLVM bitcode (`.bc`) for additional crates (UI, geisterhand).
    pub extra_bc: Vec<PathBuf>,
    /// Extra `.a` archives to add to the link line — one per
    /// well-known native binding (#466 Phase 4) that the compile
    /// pipeline routed away from the perry-stdlib copy. Whenever an
    /// entry is added here, the corresponding perry-stdlib feature
    /// is *also* stripped from the rebuild so the link line stays
    /// free of duplicate `_js_*` symbols.
    pub well_known_libs: Vec<PathBuf>,
    /// True when the stdlib archive is the prebuilt full archive rather
    /// than an optimized rebuild with well-known features stripped. In that
    /// fallback shape, wrapper archives must appear before stdlib so their
    /// duplicate Node binding symbols satisfy the object files first.
    pub prefer_well_known_before_stdlib: bool,
}

impl OptimizedLibs {
    pub(super) fn empty() -> Self {
        OptimizedLibs {
            runtime: None,
            stdlib: None,
            runtime_bc: None,
            stdlib_bc: None,
            extra_bc: Vec::new(),
            well_known_libs: Vec::new(),
            prefer_well_known_before_stdlib: false,
        }
    }
}

pub(crate) fn well_known_iteration_set(ctx: &CompilationContext) -> BTreeSet<String> {
    let mut iteration_set: BTreeSet<String> = ctx.native_module_imports.iter().cloned().collect();
    if let Ok(forced) = std::env::var("PERRY_FORCE_WELL_KNOWN") {
        for module in forced.split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace()) {
            let module = module.trim();
            if module.is_empty() {
                continue;
            }
            if super::well_known::lookup_well_known(module).is_some() {
                iteration_set.insert(module.strip_prefix("node:").unwrap_or(module).to_string());
            }
        }
    }
    iteration_set
}
