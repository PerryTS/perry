//! Compile-package Node native-addon detection.
//!
//! Extracted from `collect_modules.rs` (file-size cap). A package listed in
//! `perry.compilePackages` must be pure JS/TS — Perry cannot load Node
//! `.node` / N-API addons inside a native binary. These helpers locate the
//! package root for a resolved file and probe it for native-addon markers
//! (`binding.gyp`, `prebuilds/`, `gypfile`, `node-gyp-build`/`bindings`
//! loader deps, or a stray `*.node`), so `refuse_compile_package_native_addon`
//! can fail the compile with an actionable message instead of silently
//! emitting a broken binary.

use anyhow::Result;
use object::Object;
use std::fs;
use std::path::PathBuf;

// Parent (`collect_modules`) private imports are visible to this child module.
use super::has_perry_native_library;
use super::CompilationContext;
use crate::commands::compile::NativeAddonModule;

fn nearest_package_root(path: &std::path::Path) -> Option<PathBuf> {
    let mut dir = path.parent();
    while let Some(candidate) = dir {
        if candidate.join("package.json").exists() {
            return Some(candidate.to_path_buf());
        }
        dir = candidate.parent();
    }
    None
}

fn package_root_for_compile_package(
    ctx: &CompilationContext,
    path: &std::path::Path,
) -> Option<PathBuf> {
    ctx.compile_package_dirs
        .iter()
        .filter(|dir| path.starts_with(dir))
        .max_by_key(|dir| dir.components().count())
        .cloned()
        .or_else(|| nearest_package_root(path))
}

fn package_name_from_package_json(package_root: &std::path::Path) -> Option<String> {
    let package_json = fs::read_to_string(package_root.join("package.json")).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&package_json).ok()?;
    parsed
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

fn package_identity_from_package_json(package_root: &std::path::Path) -> Option<(String, String)> {
    let package_json = fs::read_to_string(package_root.join("package.json")).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&package_json).ok()?;
    let name = parsed.get("name")?.as_str()?.to_string();
    let version = parsed
        .get("version")
        .and_then(|value| value.as_str())
        .unwrap_or("0.0.0")
        .to_string();
    Some((name, version))
}

fn package_path(node_modules: &std::path::Path, name: &str) -> PathBuf {
    name.split('/')
        .fold(node_modules.to_path_buf(), |path, part| path.join(part))
}

/// Exact allowlisting follows the JS wrapper package through its declared
/// platform payload dependency. This covers napi-rs layouts such as
/// `@napi-rs/foo` -> `@napi-rs/foo-win32-x64-msvc` without letting an
/// unrelated transitive package inherit the permission.
fn approved_owner_package(
    ctx: &CompilationContext,
    package_root: &std::path::Path,
    actual_name: &str,
) -> Option<String> {
    if ctx.native_addon_packages.contains(actual_name) {
        return Some(actual_name.to_string());
    }
    for node_modules in package_root.ancestors().filter(|ancestor| {
        ancestor
            .file_name()
            .is_some_and(|name| name == "node_modules")
    }) {
        for allowed in &ctx.native_addon_packages {
            let manifest =
                fs::read_to_string(package_path(node_modules, allowed).join("package.json"))
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
            let declares_payload = manifest.as_ref().is_some_and(|manifest| {
                ["dependencies", "optionalDependencies"]
                    .iter()
                    .any(|section| {
                        manifest
                            .get(section)
                            .and_then(|value| value.as_object())
                            .is_some_and(|dependencies| dependencies.contains_key(actual_name))
                    })
            });
            if declares_payload {
                return Some(allowed.clone());
            }
        }
    }
    None
}

fn normalized_relative(path: &std::path::Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn forbidden_node_import(name: &str) -> bool {
    let undecorated = name.trim_start_matches('_');
    undecorated.starts_with("uv_")
        || undecorated.starts_with("ZN2v8")
        || undecorated.starts_with("ZN4node")
        || name.contains("@v8@@")
        || name.contains("@node@@")
        || name.contains("Nan::")
}

fn validate_node_api_binary(path: &std::path::Path) -> Result<()> {
    let bytes = fs::read(path)?;
    let file = object::File::parse(&*bytes).map_err(|error| {
        anyhow::anyhow!(
            "approved Node-API addon `{}` is not a supported Mach-O, ELF, or PE binary: {error}",
            path.display()
        )
    })?;
    for import in file.imports().map_err(|error| {
        anyhow::anyhow!("cannot inspect imports for `{}`: {error}", path.display())
    })? {
        let symbol = String::from_utf8_lossy(import.name());
        if forbidden_node_import(&symbol) {
            anyhow::bail!(
                "approved addon `{}` imports unsupported symbol `{}`. Perry's Node-API host supports `napi_*` / `node_api_*`, not direct libuv, V8, NAN, or Node C++ APIs.",
                path.display(),
                symbol
            );
        }
    }
    Ok(())
}

/// Record an approved `.node` graph member or emit the existing actionable
/// unsupported-addon diagnostic. Returns true exactly for `.node` inputs so
/// the caller can stop before attempting to parse the native binary.
pub(super) fn collect_or_refuse_node_addon(
    ctx: &mut CompilationContext,
    canonical: &std::path::Path,
) -> Result<bool> {
    if canonical.extension().and_then(|ext| ext.to_str()) != Some("node") {
        return Ok(false);
    }
    let package_root = nearest_package_root(canonical);
    if package_root
        .as_deref()
        .is_some_and(package_is_parcel_watcher_facade)
    {
        return Ok(true);
    }
    let Some(package_root) = package_root else {
        anyhow::bail!(
            "`{}` is a Node native addon outside an npm package. Addons must be selected through an exact `perry.nativeAddons` package entry.",
            canonical.display()
        );
    };
    let (actual_package, version) = package_identity_from_package_json(&package_root)
        .unwrap_or_else(|| (package_root.display().to_string(), "0.0.0".to_string()));
    let Some(owner_package) = approved_owner_package(ctx, &package_root, &actual_package) else {
        anyhow::bail!(
            "`{}` is a Node native addon (`{}`).\n\
             Perry executes `.node` / Node-API addons only when the host project lists the owning package in `perry.nativeAddons`. \
             Add an exact `{}` entry, choose a pure JS/TS package, or replace the native boundary with a Perry native binding (`perry.nativeLibrary` / perry-ffi).",
            actual_package,
            canonical.display(),
            actual_package,
        );
    };
    validate_node_api_binary(canonical)?;
    let entry_relative = canonical
        .strip_prefix(&package_root)
        .unwrap_or(canonical)
        .to_path_buf();
    let logical_id = format!(
        "{}/{}",
        actual_package,
        normalized_relative(&entry_relative)
    );
    ctx.native_addons
        .entry(logical_id.clone())
        .or_insert_with(|| NativeAddonModule {
            logical_id,
            package: owner_package,
            version,
            source_path: canonical.to_path_buf(),
            package_dir: package_root,
            entry_relative,
        });
    Ok(true)
}

fn package_is_parcel_watcher_facade(package_root: &std::path::Path) -> bool {
    package_name_from_package_json(package_root)
        .is_some_and(|name| name == "@parcel/watcher" || name.starts_with("@parcel/watcher-"))
}

fn find_node_addon_file(dir: &std::path::Path, max_depth: usize) -> Option<PathBuf> {
    if max_depth == 0 {
        return None;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name == "node_modules" || file_name == ".git" {
            continue;
        }
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("node") {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_node_addon_file(&path, max_depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn node_addon_marker(package_root: &std::path::Path) -> Option<(&'static str, String)> {
    if let Some(marker) = wildcard_node_addon_marker(package_root) {
        return Some(marker);
    }
    if let Some(node_file) = find_node_addon_file(package_root, 5) {
        return Some(("*.node", node_file.display().to_string()));
    }
    None
}

/// Cheap preflight used while expanding a wildcard across an entire install.
/// Reading package-level markers is bounded; recursively walking every file in
/// thousands of pure JS/TS packages would make source-first startup needlessly
/// expensive. A sidecar package containing a root-level `.node` file is still
/// detected, while the full guard retains its depth-five scan for packages
/// that are actually selected explicitly.
fn wildcard_node_addon_marker(package_root: &std::path::Path) -> Option<(&'static str, String)> {
    let binding_gyp = package_root.join("binding.gyp");
    if binding_gyp.exists() {
        return Some(("binding.gyp", binding_gyp.display().to_string()));
    }
    let prebuilds = package_root.join("prebuilds");
    if prebuilds.is_dir() {
        return Some(("prebuilds/", prebuilds.display().to_string()));
    }
    let package_json_path = package_root.join("package.json");
    if let Ok(package_json) = fs::read_to_string(&package_json_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&package_json) {
            if parsed
                .get("gypfile")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
            {
                return Some((
                    "package.json gypfile",
                    package_json_path.display().to_string(),
                ));
            }
            if package_json_dependency_uses_native_addon_loader(&parsed, "node-gyp-build")
                || package_json_dependency_uses_native_addon_loader(&parsed, "bindings")
            {
                return Some((
                    "native addon loader dependency",
                    package_json_path.display().to_string(),
                ));
            }
        }
    }
    if let Some(node_file) = find_node_addon_file(package_root, 1) {
        return Some(("*.node", node_file.display().to_string()));
    }
    None
}

/// Whether wildcard/auto `compilePackages` routing should leave this package
/// off the AOT path. Exact package opt-ins are still checked by
/// `refuse_compile_package_native_addon` and remain hard errors; this helper is
/// for broad automatic selection, where a package may be an optional
/// try/catch-guarded accelerator such as `msgpackr-extract`.
pub(in crate::commands::compile) fn package_has_unsupported_node_addon(
    package_root: &std::path::Path,
) -> bool {
    !has_perry_native_library(package_root)
        && !package_is_parcel_watcher_facade(package_root)
        && wildcard_node_addon_marker(package_root).is_some()
}

fn package_json_dependency_uses_native_addon_loader(
    package_json: &serde_json::Value,
    loader_name: &str,
) -> bool {
    ["dependencies", "optionalDependencies"]
        .iter()
        .any(|section| {
            package_json
                .get(section)
                .and_then(|deps| deps.as_object())
                .is_some_and(|deps| deps.contains_key(loader_name))
        })
}

/// A `.node` file is a compiled N-API addon — a Mach-O/ELF/PE shared object,
/// not source. Both module-read paths in `collect_modules` call
/// `fs::read_to_string`, so reaching one with a `.node` file reports
/// "stream did not contain valid UTF-8", which names neither the real
/// constraint nor the package responsible.
///
/// `refuse_compile_package_native_addon` already covers the case where the
/// addon sits in a package that resolved to a `compilePackages` root, but a
/// platform-specific sidecar package (the napi-rs layout: `@napi-rs/keyring`
/// depends on `@napi-rs/keyring-darwin-arm64`, which contains nothing but the
/// `.node` file and a package.json) can be reached without its root ever being
/// classified. Guard the read itself so the diagnostic is the same either way.
pub(super) fn refuse_compile_package_native_addon(
    ctx: &mut CompilationContext,
    canonical: &std::path::Path,
) -> Result<()> {
    let Some(package_root) = package_root_for_compile_package(ctx, canonical) else {
        return Ok(());
    };
    if !ctx
        .checked_compile_package_native_addon_roots
        .insert(package_root.clone())
    {
        return Ok(());
    }
    if has_perry_native_library(&package_root) {
        return Ok(());
    }
    if package_is_parcel_watcher_facade(&package_root) {
        return Ok(());
    }
    if package_name_from_package_json(&package_root)
        .is_some_and(|name| approved_owner_package(ctx, &package_root, &name).is_some())
    {
        return Ok(());
    }
    let Some((marker, marker_path)) = node_addon_marker(&package_root) else {
        return Ok(());
    };
    let package_name = package_name_from_package_json(&package_root)
        .unwrap_or_else(|| package_root.display().to_string());
    anyhow::bail!(
        "package `{}` is in `perry.compilePackages` but uses a Node native addon ({}) at {}.\n\
         Perry loads `.node` / Node-API addons only through an explicit host policy. \
         Add an exact `{}` entry to `perry.nativeAddons`, remove the package from \
         `perry.compilePackages`, choose a pure JS/TS package, or replace the native \
         boundary with a Perry native binding (`perry.nativeLibrary` / perry-ffi).",
        package_name,
        marker,
        marker_path,
        package_name
    );
}
