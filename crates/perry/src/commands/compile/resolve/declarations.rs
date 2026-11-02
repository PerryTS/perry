//! Declaration files that stand beside an implementation or package entry.

use super::*;

pub(super) fn canonical_existing_declaration(path: PathBuf) -> Option<PathBuf> {
    if path.exists() && is_declaration_file(&path) {
        Some(path.canonicalize().unwrap_or(path))
    } else {
        None
    }
}

pub(super) fn declaration_sidecar_for_implementation(
    implementation_path: &Path,
) -> Option<PathBuf> {
    let ext = implementation_path.extension().and_then(|e| e.to_str())?;
    let candidates: &[&str] = match ext {
        "js" => &["d.ts"],
        "mjs" => &["d.mts", "d.ts"],
        "cjs" => &["d.cts", "d.ts"],
        _ => &[],
    };
    for candidate_ext in candidates {
        if let Some(sidecar) =
            canonical_existing_declaration(implementation_path.with_extension(candidate_ext))
        {
            return Some(sidecar);
        }
    }
    None
}

pub(super) fn package_dir_for_resolved_path(
    resolved_path: &Path,
    package_name: &str,
) -> Option<PathBuf> {
    if let Some(dir) = extract_compile_package_dir(resolved_path, package_name) {
        return Some(dir);
    }

    let mut current = resolved_path.parent();
    while let Some(dir) = current {
        let pkg_json = dir.join("package.json");
        if pkg_json.exists() {
            if let Ok(content) = fs::read_to_string(&pkg_json) {
                if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                    if pkg.get("name").and_then(|v| v.as_str()) == Some(package_name) {
                        return Some(dir.to_path_buf());
                    }
                }
            }
        }
        current = dir.parent();
    }

    None
}

pub(in crate::commands::compile) fn resolve_package_declaration_entry(
    package_dir: &Path,
    subpath: Option<&str>,
    implementation_path: Option<&Path>,
) -> Option<PathBuf> {
    let package_json = package_dir.join("package.json");
    if package_json.exists() {
        let pkg = fs::read_to_string(&package_json)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());

        if let Some(pkg) = pkg {
            let export_key = if let Some(sub) = subpath {
                format!("./{}", sub)
            } else {
                ".".to_string()
            };

            if let Some(exports) = pkg.get("exports") {
                if let Some(entry) =
                    resolve_exports_with_conditions(exports, &export_key, &["types", "typings"])
                {
                    if let Some(path) = canonical_existing_declaration(package_dir.join(entry)) {
                        return Some(path);
                    }
                }
            }

            if subpath.is_none() {
                for field in ["types", "typings"] {
                    if let Some(types_path) = pkg.get(field).and_then(|v| v.as_str()) {
                        if let Some(path) =
                            canonical_existing_declaration(package_dir.join(types_path))
                        {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }

    implementation_path.and_then(declaration_sidecar_for_implementation)
}

pub(in crate::commands::compile) fn declaration_sidecar_for_resolved_import(
    import_source: &str,
    resolved_path: &Path,
) -> Option<PathBuf> {
    if is_declaration_file(resolved_path) {
        return canonical_existing_declaration(resolved_path.to_path_buf());
    }

    if !(is_relative_specifier(import_source) || is_absolute_specifier(import_source)) {
        let (package_name, subpath) = parse_package_specifier(import_source);
        if let Some(package_dir) = package_dir_for_resolved_path(resolved_path, &package_name) {
            if let Some(sidecar) = resolve_package_declaration_entry(
                &package_dir,
                subpath.as_deref(),
                Some(resolved_path),
            ) {
                return Some(sidecar);
            }
        }
    }

    declaration_sidecar_for_implementation(resolved_path)
}
