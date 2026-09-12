//! Contextual OpenTUI conditions and Solid client-build selection (#10099).

use super::*;

/// Every graph pass must use the same conditions, including re-export fixups
/// and initialization ordering which resolve edges again after collection.
pub(in crate::commands::compile) fn resolve_import_with_context(
    source: &str,
    importer: &Path,
    ctx: &CompilationContext,
) -> Option<(PathBuf, ModuleKind)> {
    let (path, kind) = resolve_import_with_bunfs(
        source,
        importer,
        &ctx.project_root,
        &ctx.compile_packages,
        &ctx.compile_package_dirs,
        ctx.bunfs_root.as_deref(),
    )?;
    let path = if ctx.bun_platform && parse_package_specifier(source).0 == "@opentui/solid" {
        opentui_bun_entry(source, &path).unwrap_or(path)
    } else {
        path
    };
    Some((
        if ctx.solid_client {
            client_entry(&path).unwrap_or(path)
        } else {
            path
        },
        kind,
    ))
}

fn opentui_bun_entry(source: &str, path: &Path) -> Option<PathBuf> {
    // Start from the resolved copy, preserving nested versions / symlink identity.
    for dir in path.parent()?.ancestors() {
        let package = dir.join("package.json");
        let Ok(content) = fs::read_to_string(package) else {
            continue;
        };
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        if json.get("name")?.as_str()? != "@opentui/solid" {
            return None;
        }
        let (_, subpath) = parse_package_specifier(source);
        let key = subpath
            .map(|s| format!("./{s}"))
            .unwrap_or_else(|| ".".into());
        let entry = resolve_exports_with_conditions(
            json.get("exports")?,
            &key,
            &["bun", "node", "import", "default"],
        )?;
        return resolve_with_extensions(&dir.join(entry))?
            .canonicalize()
            .ok();
    }
    None
}

fn client_entry(path: &Path) -> Option<PathBuf> {
    // Mirror OpenTUI's onLoad swap, including explicit/relative imports of the
    // SSR files. Never enable the browser condition for unrelated packages.
    if path.file_name()? != "server.js" || path.parent()?.file_name()? != "dist" {
        return None;
    }
    let base = path.parent()?.parent()?;
    let (package, client) = if base.file_name()? == "store" {
        (base.parent()?, "store.js")
    } else {
        (base, "solid.js")
    };
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(package.join("package.json")).ok()?).ok()?;
    if json.get("name")?.as_str()? != "solid-js" {
        return None;
    }
    path.with_file_name(client).canonicalize().ok()
}

#[cfg(test)]
mod tests;
