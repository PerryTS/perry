//! TS / JS module resolution: import paths, npm packages, file: deps,
//! perry.nativeLibrary / perry.compilePackages package discovery.
//!
//! Tier 2.1 follow-up (v0.5.340) — extracts the entire resolve_import
//! family + npm-package detection helpers + perry workspace root
//! locator from `compile.rs`. ~810 LOC of self-contained module
//! resolution logic. The fns here cover:
//!
//! - `find_perry_workspace_root` — locates the perry repo root via
//!   the executable path + workspace-marker walk (used by
//!   library_search.rs to find bundled .a files).
//! - `has_perry_native_library` / `has_perry_native_module` —
//!   classify an npm package's `perry` config block.
//! - `parse_native_library_manifest` — read the `nativeLibrary`
//!   field of an npm `package.json` into a structured manifest.
//! - `is_in_perry_native_package`, `extract_compile_package_dir`,
//!   `is_in_compile_package` — directory-membership tests for
//!   classifying resolved paths.
//! - `find_node_modules` — walk-up search.
//! - `find_file_dep_in_package_json` — resolve `"foo": "file:../bar"`
//!   shape (issue #209).
//! - `parse_package_specifier`, `resolve_with_extensions`,
//!   `resolve_package_entry`, `resolve_package_source_entry`,
//!   `resolve_exports` — the per-segment resolution logic.
//! - `resolve_import` + `cached_resolve_import` — the public entry
//!   points + cache.
//! - `discover_extension_entries`, `compute_module_prefix` —
//!   supporting helpers.

use anyhow::{anyhow, Result};
use perry_hir::ModuleKind;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::{CompilationContext, NativeFunctionDecl, NativeLibraryManifest, TargetNativeConfig};

/// Find the Perry workspace root by searching upward from the executable location.
pub fn find_perry_workspace_root() -> Option<PathBuf> {
    // First try: relative to the perry executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Binary in target/release/ → workspace is ../../
            for ancestor in [
                dir,
                &dir.join(".."),
                &dir.join("../.."),
                &dir.join("../../.."),
            ] {
                let candidate = std::fs::canonicalize(ancestor).ok()?;
                if candidate.join("crates/perry-runtime").is_dir()
                    && candidate.join("crates/perry-ui-geisterhand").is_dir()
                {
                    return Some(candidate);
                }
            }
        }
    }
    // Second try: current working directory or its ancestors
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            if dir.join("crates/perry-runtime").is_dir()
                && dir.join("crates/perry-ui-geisterhand").is_dir()
            {
                return Some(dir.to_path_buf());
            }
            dir = dir.parent()?;
        }
    }
    None
}

/// Check if a package directory has a perry.nativeLibrary field in its package.json
pub(super) fn has_perry_native_library(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg
                .get("perry")
                .and_then(|p| p.get("nativeLibrary"))
                .is_some();
        }
    }
    false
}

/// Check if a package directory has `perry.nativeModule: true` in its package.json.
///
/// Packages that set this flag contain Perry-compatible TypeScript source code
/// and should be compiled natively (NativeCompiled) rather than interpreted via V8.
/// This is the mechanism used by `perry-react`, `perry-react-dom`, and similar
/// first-party TypeScript packages that rely on `perry/ui` or other native modules.
pub(super) fn has_perry_native_module(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg
                .get("perry")
                .and_then(|p| p.get("nativeModule"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
    }
    false
}

/// Parse a native library manifest from a package's package.json
pub(super) fn parse_native_library_manifest(
    package_dir: &Path,
    module_name: &str,
    target: Option<&str>,
) -> Option<NativeLibraryManifest> {
    let package_json = package_dir.join("package.json");
    let content = fs::read_to_string(&package_json).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    let native_lib = pkg.get("perry")?.get("nativeLibrary")?;

    // Issue #466 Phase 2: read the `abiVersion` field that wrappers
    // declare to assert which `perry-ffi` ABI they were built
    // against. Strict enforcement (refuse to load on mismatch)
    // happens in `validate_abi_version` after the manifest is
    // assembled — keeping the parse loose here means we still
    // produce a structured error pointing at the package, instead
    // of silently dropping the manifest.
    let abi_version = native_lib
        .get("abiVersion")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Parse functions.
    //
    // Valid `returns` values (codegen dispatch in lower_call.rs):
    //   "string" / "ptr"  → PTR return (*const u8 / *const StringHeader);
    //                        NaN-boxed as STRING_TAG.  Use when Rust fn is
    //                        declared `-> *const u8`.
    //   "i64_str"         → I64 return that IS a *StringHeader address;
    //                        NaN-boxed as STRING_TAG without sitofp.  Use
    //                        when Rust fn is declared `-> i64` but the value
    //                        is a string pointer (closes issue #222).
    //   "i64"             → I64 return; sitofp → JS number.  Opaque handles,
    //                        counts, etc.
    //   "void"            → no return value.
    //   (anything else)   → treated as f64 (Perry double ABI).
    let functions: Vec<NativeFunctionDecl> = native_lib
        .get("functions")?
        .as_array()?
        .iter()
        .filter_map(|f| {
            Some(NativeFunctionDecl {
                name: f.get("name")?.as_str()?.to_string(),
                params: f
                    .get("params")?
                    .as_array()?
                    .iter()
                    .filter_map(|p| p.as_str().map(|s| s.to_string()))
                    .collect(),
                returns: f.get("returns")?.as_str()?.to_string(),
            })
        })
        .collect();

    // Parse target config
    let target_key = match target {
        Some("ios-simulator") | Some("ios") => "ios",
        Some("visionos-simulator") | Some("visionos") => "visionos",
        Some("android") => "android",
        Some("tvos-simulator") | Some("tvos") => "tvos",
        Some("watchos-simulator") | Some("watchos") => "watchos",
        Some("harmonyos-simulator") | Some("harmonyos") => "harmonyos",
        Some("linux") => "linux",
        Some("windows") => "windows",
        Some("web") => "web",
        None if cfg!(target_os = "linux") => "linux",
        None if cfg!(target_os = "windows") => "windows",
        _ => "macos",
    };

    // Issue #860 — prebuilt distribution (esbuild / sharp / swc /
    // lightningcss pattern) needs per-arch target keys so a single
    // package.json can describe `macos-arm64`, `macos-x64`,
    // `linux-x64`, `linux-arm64`, etc. all at once. Probe the
    // `<target>-<arch>` key first; fall back to the bare `<target>`
    // key so the existing on-disk wrappers (which only use
    // `targets.macos`, `targets.linux`, …) keep working unchanged.
    let arch_for_target = arch_for_target_key(target);
    let arch_key = arch_for_target.map(|arch| format!("{}-{}", target_key, arch));

    let targets_block = native_lib.get("targets");
    let target_value = arch_key
        .as_deref()
        .and_then(|k| targets_block.and_then(|t| t.get(k)))
        .or_else(|| targets_block.and_then(|t| t.get(target_key)));

    let target_config = target_value.map(|tc| TargetNativeConfig {
        crate_path: package_dir.join(tc.get("crate").and_then(|c| c.as_str()).unwrap_or("")),
        lib_name: tc
            .get("lib")
            .and_then(|l| l.as_str())
            .unwrap_or("")
            .to_string(),
        prebuilt: tc
            .get("prebuilt")
            .and_then(|p| p.as_str())
            .and_then(|spec| resolve_prebuilt_path(package_dir, spec)),
        frameworks: tc
            .get("frameworks")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        // Issue #1304 — vendored-SDK frameworks gated on an env var.
        // Accept both camelCase (`optionalFrameworks`/`frameworksEnv`,
        // matching `libDirs`/`pkgConfig`) and snake_case
        // (`optional_frameworks`/`frameworks_env`, matching
        // `swift_sources`/`metal_sources`) so package authors don't get
        // tripped up by the manifest's mixed casing convention.
        optional_frameworks: tc
            .get("optionalFrameworks")
            .or_else(|| tc.get("optional_frameworks"))
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        frameworks_env: tc
            .get("frameworksEnv")
            .or_else(|| tc.get("frameworks_env"))
            .and_then(|v| v.as_str())
            .map(String::from),
        libs: tc
            .get("libs")
            .and_then(|l| l.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        lib_dirs: tc
            .get("libDirs")
            .and_then(|l| l.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|p| package_dir.join(p)))
                    .collect()
            })
            .unwrap_or_default(),
        pkg_config: tc
            .get("pkgConfig")
            .and_then(|p| p.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        swift_sources: tc
            .get("swift_sources")
            .and_then(|s| s.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|p| package_dir.join(p)))
                    .collect()
            })
            .unwrap_or_default(),
        metal_sources: tc
            .get("metal_sources")
            .and_then(|s| s.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|p| package_dir.join(p)))
                    .collect()
            })
            .unwrap_or_default(),
    });

    Some(NativeLibraryManifest {
        module: module_name.to_string(),
        package_dir: package_dir.to_path_buf(),
        abi_version,
        functions,
        target_config,
    })
}

/// Map a Perry target string to the architecture token used in
/// per-arch manifest keys (e.g. `targets.macos-arm64`).
///
/// Returns `None` for targets where the architecture is implicit in
/// the target string itself (`ios` is always arm64-on-device, `web`
/// is wasm). The caller falls back to the bare OS-only target key
/// in those cases, so those wrappers don't need to migrate to the
/// per-arch shape introduced by #860.
fn arch_for_target_key(target: Option<&str>) -> Option<&'static str> {
    // Native (no `--target`): use the host arch so a per-arch entry
    // for the current machine wins over the OS-only fallback.
    if target.is_none() {
        return Some(host_arch_token());
    }
    match target {
        // OS-level targets where both arm64 and x64 are real distribution
        // targets — surface the arch so wrappers can ship per-arch
        // prebuilts.
        Some("macos") => Some("arm64"),
        Some("linux") => Some("x64"),
        Some("windows") => Some("x64"),
        Some("android") => Some("arm64"),
        Some("harmonyos") => Some("arm64"),
        Some("harmonyos-simulator") => Some("x64"),
        // ios/tvos/watchos/visionos: device builds are always arm64 (or
        // arm64_32 for watchOS). Simulators are arm64 on Apple Silicon
        // hosts and x64 on Intel hosts — we don't currently expose the
        // host distinction at the manifest level. Stick with the
        // OS-only key for now; per-arch keys can be added later if
        // wrappers start needing them.
        _ => None,
    }
}

/// Architecture token for the current host, matching what
/// `arch_for_target_key` would return for a native build. Kept in
/// sync with the npm prebuilt-distribution convention used by
/// esbuild/sharp/swc/lightningcss.
fn host_arch_token() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    }
}

/// Resolve a `prebuilt:` manifest entry to an absolute filesystem
/// path. Returns `None` if the entry could not be resolved.
///
/// Accepted shapes (issue #860):
///
/// - `./relative/path.a` or `../relative/path.a` — resolved against
///   the consuming package's directory (`package_dir`).
/// - `/abs/path.a` — used verbatim.
/// - `@scope/pkg/subpath/file.a` or `pkg/subpath/file.a` — resolved
///   as a node-style module reference. We walk up from
///   `package_dir` looking for a `node_modules/<pkg>` that contains
///   `<subpath>/<file.a>`. This matches what `require.resolve` would
///   do for a sibling package installed via npm
///   `optionalDependencies` (the esbuild/sharp pattern).
fn resolve_prebuilt_path(package_dir: &Path, spec: &str) -> Option<PathBuf> {
    if spec.is_empty() {
        return None;
    }

    let path = Path::new(spec);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    if spec.starts_with("./") || spec.starts_with("../") {
        let joined = package_dir.join(spec);
        return Some(joined);
    }

    // Node-style module reference: split off the package name (one
    // segment, or two if it starts with `@scope/`) and treat the
    // remainder as the subpath within that package.
    let (pkg_name, subpath) = split_module_spec(spec)?;

    // Walk up from `package_dir`, probing every `node_modules/<pkg>`
    // until we find a match. The optionalDependency could be installed
    // anywhere along that chain — typically right next to
    // `package_dir`'s parent (sibling under the same `node_modules/`).
    let mut current: Option<&Path> = Some(package_dir);
    while let Some(dir) = current {
        let candidate_pkg = dir.join("node_modules").join(&pkg_name);
        if candidate_pkg.is_dir() {
            let candidate = candidate_pkg.join(&subpath);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        current = dir.parent();
    }

    None
}

/// Split a node-style module reference like
/// `@scope/pkg/lib/foo.a` into `("@scope/pkg", "lib/foo.a")`.
/// Returns `None` if the spec is just a bare package name (no subpath
/// — `prebuilt:` needs to point at a specific file).
fn split_module_spec(spec: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = spec.splitn(4, '/').collect();
    if spec.starts_with('@') {
        // Scoped: `@scope/name/<rest>` — package name is first 2
        // segments, subpath is everything after.
        if parts.len() < 3 {
            return None;
        }
        let pkg = format!("{}/{}", parts[0], parts[1]);
        let subpath = parts[2..].join("/");
        Some((pkg, subpath))
    } else {
        // Unscoped: `name/<rest>`.
        if parts.len() < 2 {
            return None;
        }
        let pkg = parts[0].to_string();
        let subpath = parts[1..].join("/");
        Some((pkg, subpath))
    }
}

/// The ABI version the bundled `perry-ffi` ships. External wrappers
/// declare an `abiVersion` semver range that must include this exact
/// version to be allowed to load. Tracked alongside the workspace
/// version — `perry-ffi` ships in lockstep with `perry` itself for
/// the v0.5.x cycle.
pub const PERRY_FFI_ABI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Validate a wrapper's declared `abiVersion` against the bundled
/// `perry-ffi` version (#466 Phase 2).
///
/// Behavior on this branch (v0.5.x cycle):
/// - `None` (no field) → warning to stderr, compilation continues.
/// - Valid range that includes the bundled version → silent OK.
/// - Valid range that excludes the bundled version → error result.
/// - Unparseable string → error result.
///
/// From v0.6.0 the `None` arm flips to an error too.
pub(super) fn validate_abi_version(manifest: &NativeLibraryManifest) -> Result<(), String> {
    use semver::{Version, VersionReq};

    let bundled = Version::parse(PERRY_FFI_ABI_VERSION).map_err(|e| {
        format!(
            "internal error: bundled perry-ffi version `{}` is not valid semver: {}",
            PERRY_FFI_ABI_VERSION, e
        )
    })?;

    let Some(declared) = manifest.abi_version.as_deref() else {
        eprintln!(
            "[perry] warning: native library `{}` does not declare \
             `perry.nativeLibrary.abiVersion`. Add it to package.json \
             to assert ABI compatibility — see \
             docs/native-libraries/manifest-v1.md. (v0.5.x cycle: \
             missing field is allowed; from v0.6.0 it will be a hard error.)",
            manifest.module
        );
        return Ok(());
    };

    // Accept bare-major (`"0.5"`) and bare-minor (`"0.5.3"`) by
    // pre-pending a caret if the user didn't supply an operator.
    // Same pragma cargo's manifest parser uses for `^x.y.z` defaults.
    let req_str = if declared
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("^{}", declared)
    } else {
        declared.to_string()
    };

    let req = VersionReq::parse(&req_str).map_err(|e| {
        format!(
            "native library `{}` declares an unparseable `abiVersion: \"{}\"`: {}",
            manifest.module, declared, e
        )
    })?;

    if req.matches(&bundled) {
        Ok(())
    } else {
        Err(format!(
            "native library `{}` declares perry-ffi ABI \"{}\" but this Perry \
             build ships perry-ffi {}. Update the package or use a Perry \
             release whose perry-ffi version matches the declared range.",
            manifest.module, declared, PERRY_FFI_ABI_VERSION
        ))
    }
}

#[cfg(test)]
mod abi_validation_tests {
    use super::*;

    fn manifest_with_abi(abi: Option<&str>) -> NativeLibraryManifest {
        NativeLibraryManifest {
            module: "test".to_string(),
            package_dir: PathBuf::new(),
            abi_version: abi.map(String::from),
            functions: vec![],
            target_config: None,
        }
    }

    #[test]
    fn missing_abi_version_warns_but_passes() {
        let m = manifest_with_abi(None);
        assert!(validate_abi_version(&m).is_ok());
    }

    #[test]
    fn matching_caret_range_passes() {
        // The bundled version is whatever this build is compiled
        // against — by definition the same major.minor as itself.
        let v = PERRY_FFI_ABI_VERSION;
        let major_minor = v.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
        let m = manifest_with_abi(Some(&major_minor));
        assert!(
            validate_abi_version(&m).is_ok(),
            "wrapper declaring `{}` should validate against bundled `{}`",
            major_minor,
            v
        );
    }

    #[test]
    fn future_major_fails() {
        // `^99.0` rejects every actual perry-ffi version that ships
        // this decade. Use `^99` so we don't need to bump the test
        // when the runtime hits a multi-digit minor.
        let m = manifest_with_abi(Some("99"));
        let err = validate_abi_version(&m).expect_err("99 must reject current ABI");
        assert!(err.contains("perry-ffi"), "got: {}", err);
        assert!(err.contains("test"), "got: {}", err);
    }

    #[test]
    fn unparseable_abi_version_returns_error() {
        let m = manifest_with_abi(Some("not a version"));
        let err = validate_abi_version(&m).expect_err("garbage must reject");
        assert!(err.contains("unparseable"), "got: {}", err);
    }
}

#[cfg(test)]
mod manifest_parse_tests {
    use super::*;

    /// Relative `libDirs` entries must resolve against the package's
    /// own directory, not the user's cwd — otherwise a wrapper that
    /// ships a `vendor/lib/` alongside its `package.json` would only
    /// link when invoked from one specific directory. Absolute entries
    /// pass through unchanged (`PathBuf::join` ignores the base when
    /// the right-hand side is absolute).
    #[test]
    fn lib_dirs_relative_paths_anchored_to_package_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": {
                            "crate": "rust",
                            "lib": "demo",
                            "libDirs": ["vendor/lib", "/abs/path"]
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("macos")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.lib_dirs.len(), 2);
        assert_eq!(tc.lib_dirs[0], pkg_dir.join("vendor/lib"));
        assert_eq!(tc.lib_dirs[1], PathBuf::from("/abs/path"));
    }

    /// Omitted `libDirs` must default to an empty list, not error —
    /// it's an optional field on every existing wrapper.
    #[test]
    fn lib_dirs_defaults_to_empty_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": { "macos": { "crate": "rust", "lib": "demo" } }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("macos")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert!(tc.lib_dirs.is_empty());
    }

    /// Issue #860 — `targets.<os>-<arch>` keys take precedence over
    /// the bare `targets.<os>` key. A wrapper that ships per-arch
    /// prebuilts (esbuild/sharp/swc pattern) needs to direct macos
    /// arm64 vs macos x64 consumers at different `.a` archives even
    /// though both pass `--target macos`.
    #[test]
    fn per_arch_target_key_beats_bare_os_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos":       { "crate": "rust",       "lib": "fallback" },
                        "macos-arm64": { "crate": "rust-arm64", "lib": "arm64_lib" },
                        "macos-x64":   { "crate": "rust-x64",   "lib": "x64_lib"   }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        // The arch key for `Some("macos")` is hard-coded to `arm64`
        // by `arch_for_target_key` — that's the production macOS
        // distribution arch (Apple Silicon). x64 entries can still be
        // delivered by passing a different target string in the
        // future; we just need the per-arch lookup to fire.
        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("macos")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.lib_name, "arm64_lib");
        assert_eq!(tc.crate_path, pkg_dir.join("rust-arm64"));
    }

    /// When no per-arch key matches, the bare OS-only key still
    /// resolves — existing on-disk wrappers must not regress.
    #[test]
    fn falls_back_to_bare_os_key_when_per_arch_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": { "crate": "rust", "lib": "demo" }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("macos")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.lib_name, "demo");
        assert!(tc.prebuilt.is_none());
    }

    /// Issue #860 — `prebuilt:` pointing at a node-style module
    /// reference (`@scope/pkg/subpath/file.a`) resolves through the
    /// consumer's `node_modules`. This is the esbuild/sharp/swc
    /// distribution shape: a thin meta-package declares optional
    /// per-platform subpackages via `optionalDependencies`, npm
    /// installs only the matching one, and `prebuilt:` reaches into
    /// it without invoking cargo.
    #[test]
    fn prebuilt_resolves_node_modules_subpackage() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        // Lay out a realistic node_modules: consumer/node_modules/
        // @bloomengine/{engine, engine-darwin-arm64}/.
        let consumer_pkg = root
            .join("node_modules")
            .join("@bloomengine")
            .join("engine");
        let prebuilt_pkg = root
            .join("node_modules")
            .join("@bloomengine")
            .join("engine-darwin-arm64")
            .join("lib");
        std::fs::create_dir_all(&consumer_pkg).expect("mkdir engine");
        std::fs::create_dir_all(&prebuilt_pkg).expect("mkdir engine-darwin-arm64/lib");
        let prebuilt_file = prebuilt_pkg.join("libbloom_macos.a");
        std::fs::write(&prebuilt_file, b"fake archive").expect("write prebuilt");

        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos-arm64": {
                            "prebuilt": "@bloomengine/engine-darwin-arm64/lib/libbloom_macos.a",
                            "frameworks": ["Metal", "QuartzCore"]
                        }
                    }
                }
            }
        });
        std::fs::write(
            consumer_pkg.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write engine/package.json");

        let parsed =
            parse_native_library_manifest(&consumer_pkg, "@bloomengine/engine", Some("macos"))
                .expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        let prebuilt = tc.prebuilt.expect("prebuilt path");
        // Use canonicalize on both sides — the test's `tmpdir` on
        // macOS lives under `/var/...` which is a symlink to
        // `/private/var/...`; the resolver returns the symlinked
        // form, the original `prebuilt_file` was constructed with
        // the symlinked form too, so they match before canonicalize
        // here. But canonicalize defensively in case CI tmpdirs differ.
        assert_eq!(
            prebuilt.canonicalize().expect("canonicalize prebuilt"),
            prebuilt_file.canonicalize().expect("canonicalize expected")
        );
        assert_eq!(tc.frameworks, vec!["Metal", "QuartzCore"]);
        // The cargo build path should still be empty — no `crate:`
        // means the prebuilt branch is exclusive.
        assert_eq!(tc.lib_name, "");
    }

    /// Relative `prebuilt:` paths anchor against the package's own
    /// directory — useful for tarball-shipped wrappers that vendor
    /// the static lib alongside their `package.json`.
    #[test]
    fn prebuilt_relative_path_anchors_to_package_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let vendor_dir = pkg_dir.join("vendor");
        std::fs::create_dir_all(&vendor_dir).expect("mkdir vendor");
        let lib_path = vendor_dir.join("libfoo.a");
        std::fs::write(&lib_path, b"fake archive").expect("write lib");

        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "macos": { "prebuilt": "./vendor/libfoo.a" }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("macos")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        let prebuilt = tc.prebuilt.expect("prebuilt path");
        assert_eq!(prebuilt, pkg_dir.join("./vendor/libfoo.a"));
    }

    /// Issue #1304 — vendored-SDK frameworks parse from the snake_case
    /// manifest keys (`optional_frameworks` / `frameworks_env`), matching
    /// the `swift_sources` / `metal_sources` convention. These are the
    /// shape `@perryts/google-auth` uses for the real GoogleSignIn SDK.
    #[test]
    fn optional_frameworks_parse_snake_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "ios": {
                            "crate": "crate-ios",
                            "lib": "perry_google_auth",
                            "optional_frameworks": ["GoogleSignIn"],
                            "frameworks_env": "PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR"
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("ios")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.optional_frameworks, vec!["GoogleSignIn"]);
        assert_eq!(
            tc.frameworks_env.as_deref(),
            Some("PERRY_GOOGLE_SIGN_IN_FRAMEWORK_DIR")
        );
    }

    /// The camelCase spelling (`optionalFrameworks` / `frameworksEnv`,
    /// matching `libDirs` / `pkgConfig`) is accepted too — the manifest's
    /// casing convention is mixed, so we don't want a silent no-op when an
    /// author picks the camelCase form.
    #[test]
    fn optional_frameworks_parse_camel_case() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": {
                        "ios": {
                            "crate": "crate-ios",
                            "lib": "demo",
                            "optionalFrameworks": ["GoogleSignIn", "AppAuth"],
                            "frameworksEnv": "VENDOR_FW_DIR"
                        }
                    }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("ios")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert_eq!(tc.optional_frameworks, vec!["GoogleSignIn", "AppAuth"]);
        assert_eq!(tc.frameworks_env.as_deref(), Some("VENDOR_FW_DIR"));
    }

    /// Omitting both fields must default to an empty list / `None`, not
    /// error — every existing wrapper lacks them.
    #[test]
    fn optional_frameworks_default_when_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pkg_dir = dir.path();
        let manifest = serde_json::json!({
            "perry": {
                "nativeLibrary": {
                    "functions": [],
                    "targets": { "ios": { "crate": "rust", "lib": "demo" } }
                }
            }
        });
        std::fs::write(
            pkg_dir.join("package.json"),
            serde_json::to_string(&manifest).unwrap(),
        )
        .expect("write package.json");

        let parsed =
            parse_native_library_manifest(pkg_dir, "demo", Some("ios")).expect("parsed manifest");
        let tc = parsed.target_config.expect("target_config");
        assert!(tc.optional_frameworks.is_empty());
        assert!(tc.frameworks_env.is_none());
    }
}

#[cfg(test)]
mod module_spec_tests {
    use super::split_module_spec;

    #[test]
    fn splits_scoped_package_and_subpath() {
        let (pkg, sub) = split_module_spec("@bloomengine/engine-darwin-arm64/lib/libbloom_macos.a")
            .expect("split");
        assert_eq!(pkg, "@bloomengine/engine-darwin-arm64");
        assert_eq!(sub, "lib/libbloom_macos.a");
    }

    #[test]
    fn splits_unscoped_package_and_subpath() {
        let (pkg, sub) = split_module_spec("esbuild-darwin-arm64/bin/esbuild").expect("split");
        assert_eq!(pkg, "esbuild-darwin-arm64");
        assert_eq!(sub, "bin/esbuild");
    }

    #[test]
    fn bare_scoped_package_without_subpath_rejected() {
        // `@scope/pkg` has no file to link — `prebuilt:` must name a
        // specific archive within the package.
        assert!(split_module_spec("@bloomengine/engine-darwin-arm64").is_none());
    }

    #[test]
    fn bare_unscoped_package_without_subpath_rejected() {
        assert!(split_module_spec("esbuild-darwin-arm64").is_none());
    }
}

/// Packages that Perry provides built-in native extensions for.
/// These must never be loaded into V8 — Perry's codegen intercepts all imports
/// from these packages and replaces them with native calls.
const PERRY_NATIVE_EXTENSION_PACKAGES: &[&str] = &["ioredis", "ethers", "mysql2", "ws", "dotenv"];

/// Check if a file path is inside a Perry native extension package (has built-in stdlib support)
/// or a package that has perry.nativeLibrary in its package.json.
pub(super) fn is_in_perry_native_package(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    // Check hardcoded native extension packages first (fast path)
    for pkg_name in PERRY_NATIVE_EXTENSION_PACKAGES {
        let needle_slash = format!("node_modules/{}/", pkg_name);
        let needle_end = format!("node_modules/{}", pkg_name);
        if path_str.contains(&needle_slash) || path_str.ends_with(&needle_end) {
            return true;
        }
    }
    // Fall back to package.json perry.nativeLibrary check
    let mut current = path.parent();
    while let Some(dir) = current {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            return has_perry_native_library(dir);
        }
        // Stop at node_modules boundary
        if dir
            .file_name()
            .map(|n| n == "node_modules")
            .unwrap_or(false)
        {
            break;
        }
        current = dir.parent();
    }
    false
}

/// Extract the package directory from a resolved path for a given package name.
/// E.g., for path "/project/node_modules/@noble/curves/node_modules/@noble/hashes/src/sha256.ts"
/// and package_name "@noble/hashes", returns "/project/node_modules/@noble/curves/node_modules/@noble/hashes"
pub(super) fn extract_compile_package_dir(
    resolved_path: &Path,
    package_name: &str,
) -> Option<PathBuf> {
    let path_str = resolved_path.to_string_lossy();
    let needle = format!("node_modules/{}", package_name);
    // Use rfind to handle deeply nested node_modules
    path_str
        .rfind(&needle)
        .map(|idx| PathBuf::from(&path_str[..idx + needle.len()]))
}

/// Check if a file path is inside a package listed in compile_packages
pub(super) fn is_in_compile_package(path: &Path, compile_packages: &HashSet<String>) -> bool {
    let path_str = path.to_string_lossy();
    for pkg_name in compile_packages {
        let pattern = format!("node_modules/{}/", pkg_name);
        if path_str.contains(&pattern) {
            return true;
        }
    }
    false
}

/// Find node_modules directory starting from a given path
pub(super) fn find_node_modules(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let node_modules = current.join("node_modules");
        if node_modules.is_dir() {
            return Some(node_modules);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Look up a bare package name in the nearest package.json's `dependencies` /
/// `devDependencies` sections and, if the entry has a `file:` prefix, return the
/// resolved directory path (NOT canonicalized — caller does that).
///
/// This is the fallback used when `node_modules/<pkg>` does not exist (e.g., the
/// user manually removed the symlink, or `npm install` was not re-run after
/// rewriting `package.json` to point at a new `file:` path).  It also covers
/// the "file: dep inside the project root" shape described in #209:
///
///   "bloom": "file:./vendor/bloom/"   ← vendor/bloom may itself be a symlink
///
/// By resolving against the package.json directory (not through the node_modules
/// symlink chain) we arrive at the same canonical target regardless of how many
/// symlink hops npm left behind.
pub(super) fn find_file_dep_in_package_json(start: &Path, package_name: &str) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            if let Ok(content) = fs::read_to_string(&pkg_json) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    for dep_section in &["dependencies", "devDependencies"] {
                        if let Some(deps) = pkg.get(*dep_section).and_then(|d| d.as_object()) {
                            if let Some(dep_val) = deps.get(package_name) {
                                if let Some(dep_str) = dep_val.as_str() {
                                    if let Some(file_path) = dep_str.strip_prefix("file:") {
                                        // Trim trailing slash so dir.join() works cleanly
                                        let resolved = dir.join(file_path.trim_end_matches('/'));
                                        return Some(resolved);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // Found a package.json but no matching file: dep for this package.
            // Stop climbing — don't look in ancestor workspaces.
            break;
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Parse a package specifier into (package_name, subpath)
pub(super) fn parse_package_specifier(specifier: &str) -> (String, Option<String>) {
    if specifier.starts_with('@') {
        // Scoped package: @scope/package or @scope/package/subpath
        let parts: Vec<&str> = specifier.splitn(3, '/').collect();
        if parts.len() >= 2 {
            let package_name = format!("{}/{}", parts[0], parts[1]);
            let subpath = if parts.len() > 2 {
                Some(parts[2].to_string())
            } else {
                None
            };
            return (package_name, subpath);
        }
    } else {
        // Regular package: package or package/subpath
        let parts: Vec<&str> = specifier.splitn(2, '/').collect();
        let package_name = parts[0].to_string();
        let subpath = if parts.len() > 1 {
            Some(parts[1].to_string())
        } else {
            None
        };
        return (package_name, subpath);
    }

    (specifier.to_string(), None)
}

/// Try to resolve a path with common extensions
/// Prefers TypeScript source files over JavaScript for native compilation
pub(super) fn resolve_with_extensions(base: &Path) -> Option<PathBuf> {
    // TypeScript extensions to try (in order of preference)
    let ts_extensions = [".ts", ".tsx", ".mts"];
    // JavaScript extensions (fallback)
    let _js_extensions = [".js", ".mjs", ".cjs"];
    // All extensions in order of preference
    let all_extensions = [".ts", ".tsx", ".mts", ".js", ".mjs", ".cjs", ".json"];

    // Check if the path has an explicit JS extension - if so, try TS equivalents first
    if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
        if matches!(ext, "js" | "mjs" | "cjs") {
            // Strip the JS extension and try TS extensions first
            let stem = base.with_extension("");
            for ts_ext in ts_extensions {
                let ts_path = stem.with_extension(ts_ext.trim_start_matches('.'));
                if ts_path.exists() && ts_path.is_file() {
                    return Some(ts_path);
                }
            }
            // If no TS file found, fall back to the original JS file
            if base.exists() && base.is_file() {
                return Some(base.to_path_buf());
            }
        }
    }

    // If it already exists as-is (and not a JS file that we already handled above)
    if base.exists() && base.is_file() {
        // Even if it exists, check for TS version first
        if let Some(ext) = base.extension().and_then(|e| e.to_str()) {
            if matches!(ext, "js" | "mjs" | "cjs") {
                let stem = base.with_extension("");
                for ts_ext in ts_extensions {
                    let ts_path = stem.with_extension(ts_ext.trim_start_matches('.'));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
        }
        return Some(base.to_path_buf());
    }

    // Try with extensions in order of preference (TS before JS)
    for ext in all_extensions {
        let with_ext = base.with_extension(ext.trim_start_matches('.'));
        if with_ext.exists() && with_ext.is_file() {
            return Some(with_ext);
        }

        // Also try adding extension to full path (for paths like ./foo.js)
        let path_str = base.to_string_lossy();
        let with_ext = PathBuf::from(format!("{}{}", path_str, ext));
        if with_ext.exists() && with_ext.is_file() {
            // If we found a JS file, check for TS equivalent first
            if matches!(ext, ".js" | ".mjs" | ".cjs") {
                let stem_str = path_str.to_string();
                for ts_ext in ts_extensions {
                    let ts_path = PathBuf::from(format!("{}{}", stem_str, ts_ext));
                    if ts_path.exists() && ts_path.is_file() {
                        return Some(ts_path);
                    }
                }
            }
            return Some(with_ext);
        }
    }

    // Try index files in directory
    if base.is_dir() {
        for ext in all_extensions {
            let index = base.join(format!("index{}", ext));
            if index.exists() {
                return Some(index);
            }
        }
    }

    None
}

/// Resolve package.json entry point
pub(super) fn resolve_package_entry(package_dir: &Path, subpath: Option<&str>) -> Option<PathBuf> {
    let package_json = package_dir.join("package.json");
    if !package_json.exists() {
        // Fall back to index.js
        return resolve_with_extensions(&package_dir.join("index"));
    }

    let content = fs::read_to_string(&package_json).ok()?;
    let pkg: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try "exports" field first (modern packages), for both main and subpaths
    let export_key = if let Some(sub) = subpath {
        format!("./{}", sub)
    } else {
        ".".to_string()
    };

    if let Some(exports) = pkg.get("exports") {
        if let Some(entry) = resolve_exports(exports, &export_key) {
            let entry_path = package_dir.join(&entry);
            if entry_path.exists() {
                return Some(entry_path);
            }
        }
    }

    // If there's a subpath and exports didn't match, resolve it directly
    if let Some(sub) = subpath {
        let subpath_resolved = package_dir.join(sub);
        return resolve_with_extensions(&subpath_resolved);
    }

    // Try "types" or "typings" field for TypeScript
    for field in ["types", "typings"] {
        if let Some(types_path) = pkg.get(field).and_then(|v| v.as_str()) {
            // Look for corresponding .ts file
            let types_file = package_dir.join(types_path);
            let ts_file = types_file.with_extension("ts");
            // Skip .d.ts declaration files - they're type-only, not real source
            if ts_file.exists() && !ts_file.to_string_lossy().ends_with(".d.ts") {
                return Some(ts_file);
            }
        }
    }

    // Try "module" field (ESM)
    if let Some(module) = pkg.get("module").and_then(|v| v.as_str()) {
        let module_path = package_dir.join(module);
        if module_path.exists() {
            return Some(module_path);
        }
    }

    // Try "main" field (CommonJS)
    if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
        let main_path = package_dir.join(main);
        return resolve_with_extensions(&main_path);
    }

    // Fall back to index files
    resolve_with_extensions(&package_dir.join("index"))
}

/// Resolve package entry preferring TypeScript source over compiled JS output.
/// Used for compile_packages where we want to compile from TS source, not bundled JS.
pub(super) fn resolve_package_source_entry(
    package_dir: &Path,
    subpath: Option<&str>,
) -> Option<PathBuf> {
    // For subpaths, try src/<subpath>.ts
    if let Some(sub) = subpath {
        let src_path = package_dir.join("src").join(sub);
        if let Some(resolved) = resolve_with_extensions(&src_path) {
            if !is_js_file(&resolved) {
                return Some(resolved);
            }
        }
    }

    // Try src/index.ts (most common TS source entry)
    let src_index = package_dir.join("src").join("index");
    if let Some(resolved) = resolve_with_extensions(&src_index) {
        if !is_js_file(&resolved) {
            return Some(resolved);
        }
    }

    // Try using normal entry resolution but prefer TS over JS
    let normal_entry = resolve_package_entry(package_dir, subpath)?;
    if is_js_file(&normal_entry) {
        // Try .ts equivalent of the .js entry
        let ts_path = normal_entry.with_extension("ts");
        if ts_path.exists() {
            return Some(ts_path);
        }
        // Check src/ directory mirror of lib/ or dist/ path
        if let Ok(rel) = normal_entry.strip_prefix(package_dir) {
            let rel_str = rel.to_string_lossy();
            if rel_str.starts_with("lib") || rel_str.starts_with("dist") {
                let stripped = if rel_str.starts_with("lib") {
                    rel.strip_prefix("lib")
                } else {
                    rel.strip_prefix("dist")
                };
                if let Ok(rest) = stripped {
                    let src_equiv = package_dir.join("src").join(rest).with_extension("ts");
                    if src_equiv.exists() {
                        return Some(src_equiv);
                    }
                }
            }
        }
    }

    None
}

/// Resolve exports field from package.json
pub(super) fn resolve_exports(exports: &serde_json::Value, subpath: &str) -> Option<String> {
    match exports {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(map) => {
            // Try the specific subpath first
            if let Some(entry) = map.get(subpath) {
                return resolve_exports(entry, subpath);
            }

            // Try wildcard patterns (e.g., "./*" -> "./src/*.ts")
            for (key, value) in map.iter() {
                if key.contains('*') {
                    // Convert "./*" to a prefix/suffix match
                    let parts: Vec<&str> = key.splitn(2, '*').collect();
                    if parts.len() == 2 {
                        let prefix = parts[0];
                        let suffix = parts[1];
                        if subpath.starts_with(prefix) && subpath.ends_with(suffix) {
                            let matched = &subpath[prefix.len()..subpath.len() - suffix.len()];
                            if let Some(template) = resolve_exports(value, subpath) {
                                return Some(template.replace('*', matched));
                            }
                        }
                    }
                }
            }

            // Try common conditions (for both main entry and subpath entries)
            // This handles the case where we've matched a subpath and now need to resolve the conditions.
            // "perry" is checked first so packages can ship a TypeScript source entry
            // intended for Perry compilation alongside a pre-built JS entry for Node/Bun.
            for condition in ["perry", "import", "module", "default", "require", "node"] {
                if let Some(entry) = map.get(condition) {
                    return resolve_exports(entry, subpath);
                }
            }

            None
        }
        _ => None,
    }
}

/// Determine if a file is a JavaScript file (not TypeScript)
pub(super) fn is_js_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "js" | "mjs" | "cjs")
    } else {
        false
    }
}

/// Determine if a file is a TypeScript declaration file (.d.ts)
pub(super) fn is_declaration_file(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".d.ts")
}

/// Determine if a file is a TypeScript file (but not a declaration file)
pub(super) fn is_ts_file(path: &Path) -> bool {
    if is_declaration_file(path) {
        return false;
    }
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(ext, "ts" | "tsx")
    } else {
        false
    }
}

/// Resolve an import specifier to a file path
pub(super) fn resolve_import(
    import_source: &str,
    importer_path: &Path,
    project_root: &Path,
    compile_packages: &HashSet<String>,
    compile_package_dirs: &HashMap<String, PathBuf>,
) -> Option<(PathBuf, ModuleKind)> {
    // Check if it's a native Rust stdlib module. Refs #665: when the user has
    // explicitly opted the package into `perry.compilePackages`, they want
    // their `node_modules` copy compiled from source (cjs_wrap + native
    // codegen), not the built-in Rust FFI binding — which for some packages
    // (e.g. `rate-limiter-flexible`'s `perry-ext-ratelimit`) is incomplete.
    // The opt-in is package-scoped: bare `rate-limiter-flexible` and any
    // subpath under it both fall through to file resolution.
    let (native_check_pkg, _) = parse_package_specifier(import_source);
    if perry_hir::is_native_module(import_source) && !compile_packages.contains(&native_check_pkg) {
        return None; // Native modules are handled by stdlib, not file imports
    }

    // Handle relative imports (./ or ../)
    if import_source.starts_with("./") || import_source.starts_with("../") {
        let parent = importer_path.parent()?;
        let resolved = parent.join(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let canonical = path.canonicalize().ok()?;
            // Refs #486: a relative `import './foo.js'` from inside a compile
            // package must classify as NativeCompiled even when the resolved
            // file lives outside the literal `node_modules/<pkg>/` substring
            // — `file:./lib3` deps and symlinked package roots both canonicalize
            // away from `node_modules`, but their files are still part of the
            // compile-package compile scope. Without this, re-exports inside
            // such packages (e.g. `lib3/index.js` doing `export { C } from
            // './c.js'`) silently fall through to ModuleKind::Interpreted, the
            // dependent file never enters `ctx.native_modules`, and importing
            // modules see `imported_classes=[]` for symbols re-exported from it.
            let in_compile_pkg = is_in_compile_package(&canonical, compile_packages)
                || compile_package_dirs.values().any(|dir| {
                    if canonical.starts_with(dir) {
                        let relative = canonical.strip_prefix(dir).unwrap_or(canonical.as_path());
                        !relative.to_string_lossy().contains("node_modules/")
                    } else {
                        false
                    }
                });
            let kind = if is_js_file(&canonical) && !in_compile_pkg {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((canonical, kind));
        }
        return None;
    }

    // Handle absolute paths
    if import_source.starts_with('/') {
        let resolved = PathBuf::from(import_source);
        if let Some(path) = resolve_with_extensions(&resolved) {
            let kind = if is_js_file(&path) {
                ModuleKind::Interpreted
            } else {
                ModuleKind::NativeCompiled
            };
            return Some((path.canonicalize().ok()?, kind));
        }
        return None;
    }

    // Handle node_modules (bare specifiers)
    let (package_name, subpath) = parse_package_specifier(import_source);

    // For compile_packages, search project root first to prefer ESM versions
    // over nested CJS copies (e.g., @solana/web3.js/node_modules/bs58 is CJS,
    // but the top-level node_modules/bs58 has ESM support)
    let search_paths = if compile_packages.contains(&package_name) {
        [Some(project_root), importer_path.parent()]
    } else {
        [importer_path.parent(), Some(project_root)]
    };

    for start in search_paths.iter().flatten() {
        if let Some(node_modules) = find_node_modules(start) {
            let package_dir = node_modules.join(&package_name);
            if package_dir.is_dir() {
                if let Some(entry) = resolve_package_entry(&package_dir, subpath.as_deref()) {
                    // Packages with perry.nativeLibrary are compiled natively (Rust FFI)
                    if has_perry_native_library(&package_dir) {
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // Packages with perry.nativeModule: true contain Perry-compatible
                    // TypeScript that must be compiled natively (e.g. perry-react).
                    if has_perry_native_module(&package_dir) {
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // Packages listed in perry.compilePackages are compiled natively
                    if compile_packages.contains(&package_name) {
                        // Deduplicate: if we've already resolved this package from a
                        // different node_modules location, use the first-found directory
                        // to avoid duplicate symbols from identical package copies
                        let effective_dir = compile_package_dirs
                            .get(&package_name)
                            .unwrap_or(&package_dir);
                        // Prefer TypeScript source over compiled JS
                        if let Some(src_entry) =
                            resolve_package_source_entry(effective_dir, subpath.as_deref())
                        {
                            return Some((
                                src_entry.canonicalize().ok()?,
                                ModuleKind::NativeCompiled,
                            ));
                        }
                        // Fall back to normal resolution but still mark as NativeCompiled
                        if let Some(fallback_entry) =
                            resolve_package_entry(effective_dir, subpath.as_deref())
                        {
                            return Some((
                                fallback_entry.canonicalize().ok()?,
                                ModuleKind::NativeCompiled,
                            ));
                        }
                        // If effective_dir failed (shouldn't happen), try the local dir
                        return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    // For other node_modules packages, classify by file
                    // extension. `.ts` / `.tsx` sources are compiled natively
                    // (matches what `collect_modules` already does for the
                    // recursive walk when `enable_js_runtime` is off — the
                    // default). `.js` / `.mjs` / `.cjs` and other shapes
                    // stay Interpreted so V8 / QuickJS handles them at
                    // runtime.
                    let canonical = entry.canonicalize().ok()?;
                    let kind = if is_ts_file(&canonical) {
                        ModuleKind::NativeCompiled
                    } else {
                        ModuleKind::Interpreted
                    };
                    return Some((canonical, kind));
                }
            }
        }
    }

    // Fallback: look for a `file:` entry in the nearest package.json.
    //
    // Handles two failure modes that the node_modules walk above cannot catch:
    //
    //   1. `node_modules/<pkg>` was removed (or npm install was not re-run after
    //      changing package.json).  The manual repro in #209 hits this directly.
    //
    //   2. `node_modules/<pkg>` exists but points *inside* the project root via an
    //      intermediate symlink (e.g. `node_modules/bloom -> ../vendor/bloom` where
    //      `vendor/bloom` is itself a symlink or a real directory cloned by CI).
    //      In that case the canonical path resolves to a path like
    //      `/project/vendor/bloom/index.ts` — which is inside the project root but
    //      outside any `node_modules/` component — so the `is_in_node_modules`
    //      string check returns false and downstream classify-as-Interpreted guards
    //      can misfire for JS files.  Resolving directly from `package.json` gives
    //      us the same canonical target while keeping `package_dir` pointing at the
    //      real package root (with its perry.nativeLibrary / perry.nativeModule
    //      marker) so `has_perry_native_library` can read it without traversing a
    //      potentially-confusing symlink chain.
    if let Some(file_dep_dir) = find_file_dep_in_package_json(project_root, &package_name) {
        if file_dep_dir.is_dir() {
            if let Some(entry) = resolve_package_entry(&file_dep_dir, subpath.as_deref()) {
                if has_perry_native_library(&file_dep_dir) {
                    return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                }
                if has_perry_native_module(&file_dep_dir) {
                    return Some((entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                }
                if compile_packages.contains(&package_name) {
                    if let Some(src_entry) =
                        resolve_package_source_entry(&file_dep_dir, subpath.as_deref())
                    {
                        return Some((src_entry.canonicalize().ok()?, ModuleKind::NativeCompiled));
                    }
                    if let Some(fallback_entry) =
                        resolve_package_entry(&file_dep_dir, subpath.as_deref())
                    {
                        return Some((
                            fallback_entry.canonicalize().ok()?,
                            ModuleKind::NativeCompiled,
                        ));
                    }
                }
                // Same `.ts` / `.tsx` → NativeCompiled rule as the
                // node_modules fallback above. Keeps `file:` deps that
                // ship TypeScript source aligned with `collect_modules`'s
                // recursive native walk.
                let canonical = entry.canonicalize().ok()?;
                let kind = if is_ts_file(&canonical) {
                    ModuleKind::NativeCompiled
                } else {
                    ModuleKind::Interpreted
                };
                return Some((canonical, kind));
            }
        }
    }

    None
}

/// Discover extension entry points from a directory of plugins.
/// Each subdirectory is checked for a package.json with an `openclaw.extensions` array.
/// Returns Vec<(entry_path, plugin_id)> — e.g., ("extensions/telegram/index.ts", "telegram").
pub(super) fn discover_extension_entries(dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut entries = Vec::new();

    if !dir.is_dir() {
        return Err(anyhow!(
            "--bundle-extensions path is not a directory: {}",
            dir.display()
        ));
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let subdir = entry.path();
        if !subdir.is_dir() {
            continue;
        }

        let plugin_id = subdir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let pkg_json_path = subdir.join("package.json");
        if pkg_json_path.exists() {
            // Read package.json and look for openclaw.extensions
            let pkg_contents = fs::read_to_string(&pkg_json_path)
                .map_err(|e| anyhow!("Failed to read {}: {}", pkg_json_path.display(), e))?;
            let pkg: serde_json::Value = serde_json::from_str(&pkg_contents)
                .map_err(|e| anyhow!("Failed to parse {}: {}", pkg_json_path.display(), e))?;

            let extensions = pkg
                .get("openclaw")
                .and_then(|oc| oc.get("extensions"))
                .and_then(|ext| ext.as_array());

            if let Some(ext_array) = extensions {
                for ext_entry in ext_array {
                    if let Some(rel_path) = ext_entry.as_str() {
                        let entry_path = subdir.join(rel_path.trim_start_matches("./"));
                        if entry_path.exists() {
                            entries.push((entry_path, plugin_id.clone()));
                        }
                    }
                }
            } else {
                // Fallback: look for index.ts
                let index_path = subdir.join("index.ts");
                if index_path.exists() {
                    entries.push((index_path, plugin_id));
                }
            }
        } else {
            // No package.json — try index.ts directly
            let index_path = subdir.join("index.ts");
            if index_path.exists() {
                entries.push((index_path, plugin_id));
            }
        }
    }

    // Sort for deterministic ordering
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Compute a sanitized module prefix from a resolved path for scoped cross-module symbols
pub(super) fn compute_module_prefix(resolved_path: &str, project_root: &Path) -> String {
    let source_path = PathBuf::from(resolved_path);
    let source_module_name = source_path
        .strip_prefix(project_root)
        .ok()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            source_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("module")
                .to_string()
        });
    let mut prefix = source_module_name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
    // LLVM IR identifiers cannot start with a digit. Prefix with `_`
    // if the first character would be one (e.g. `05_fibonacci.ts`).
    if prefix
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        prefix.insert(0, '_');
    }
    prefix
}

/// Cached wrapper around resolve_import to avoid redundant I/O
pub(super) fn cached_resolve_import(
    import_source: &str,
    importer_path: &Path,
    ctx: &mut CompilationContext,
) -> Option<(PathBuf, ModuleKind)> {
    let importer_dir = importer_path
        .parent()
        .unwrap_or(importer_path)
        .to_path_buf();
    let cache_key = (import_source.to_string(), importer_dir);
    if let Some(cached) = ctx.resolve_cache.get(&cache_key) {
        return cached.clone();
    }
    let result = resolve_import(
        import_source,
        importer_path,
        &ctx.project_root,
        &ctx.compile_packages,
        &ctx.compile_package_dirs,
    );
    ctx.resolve_cache.insert(cache_key, result.clone());
    result
}
