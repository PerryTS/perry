//! Declared native inputs do not need a literal JavaScript import edge.
use super::*;
use object::Object;
use std::collections::BTreeSet;
use std::path::Path;

pub(in crate::commands::compile) fn collect_declared_addons(
    ctx: &mut CompilationContext,
    target: Option<&str>,
) -> Result<()> {
    if ctx.native_addon_packages.is_empty() && ctx.native_addon_paths.is_empty() {
        return Ok(());
    }
    let triple = crate::commands::compile::rust_target_triple(target)
        .or_else(crate::commands::compile::host_target_triple)
        .ok_or_else(|| anyhow::anyhow!("cannot determine native addon target"))?;
    for path in ctx.native_addon_paths.keys().cloned().collect::<Vec<_>>() {
        anyhow::ensure!(
            matches_target(&path, triple)?,
            "declared addon {} does not match target {triple}",
            path.display()
        );
        collect_node_addon_request(ctx, &path)?;
    }
    // Only packages actually reached by compilation participate. An unused
    // allowlist entry must not turn an ordinary program into a Node-API host.
    let mut reached = ctx.checked_compile_package_native_addon_roots.clone();
    reached.extend(
        ctx.native_modules
            .keys()
            .filter_map(|path| nearest_package_root(path)),
    );
    let mut required = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for root in &reached {
        let Some(name) = package_name_from_package_json(root) else {
            continue;
        };
        if approved_owner_package(ctx, root, &name).is_none() || has_perry_native_library(root) {
            continue;
        }
        roots.insert(root.clone());
        if node_addon_marker(root).is_some() {
            required.insert(approved_owner_package(ctx, root, &name).unwrap());
        }
        if !ctx.native_addon_packages.contains(&name) {
            continue;
        }
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("package.json"))?)?;
        for section in ["dependencies", "optionalDependencies"] {
            let Some(deps) = manifest.get(section).and_then(|v| v.as_object()) else {
                continue;
            };
            for dependency in deps.keys() {
                for ancestor in root.ancestors() {
                    let candidate = package_path(&ancestor.join("node_modules"), dependency);
                    if candidate.join("package.json").is_file() {
                        roots.insert(candidate.canonicalize()?);
                        break;
                    }
                }
            }
        }
    }
    for root in roots {
        if has_perry_native_library(&root) {
            continue;
        }
        let mut candidates = Vec::new();
        addon_files(&root, &root, &mut candidates)?;
        candidates.sort();
        if !candidates.is_empty() {
            if let Some(owner) = package_name_from_package_json(&root)
                .and_then(|name| approved_owner_package(ctx, &root, &name))
            {
                required.insert(owner);
            }
        }
        for path in candidates {
            if matches_target(&path, triple)? {
                collect_node_addon_request(ctx, &path)?;
            }
        }
    }
    for owner in required {
        anyhow::ensure!(ctx.native_addons.values().any(|addon| addon.package == owner),
            "approved package `{owner}` has no Node-API addon matching target `{triple}`. Install or build its target-specific .node binary before compiling");
    }
    Ok(())
}

fn addon_files(root: &Path, dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name() == "node_modules" || entry.file_name() == ".git" {
            continue;
        }
        // Do not follow directory symlinks, cycles, or files outside this package.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            addon_files(root, &entry.path(), output)?;
        } else if entry.path().extension().and_then(|s| s.to_str()) == Some("node") {
            let path = entry.path().canonicalize()?;
            anyhow::ensure!(
                path.starts_with(root),
                "native addon {} escapes package {}",
                path.display(),
                root.display()
            );
            output.push(path);
        }
    }
    Ok(())
}

/// True when `path` is a Node-API binary built for a known platform other
/// than the compile target. A file that cannot be inspected is not claimed,
/// so the regular validation reports it.
pub(super) fn is_other_target_addon(path: &Path, target: Option<&str>) -> bool {
    let Some(triple) = crate::commands::compile::rust_target_triple(target)
        .or_else(crate::commands::compile::host_target_triple)
    else {
        return false;
    };
    (triple.starts_with("aarch64-") || triple.starts_with("x86_64-"))
        && matches!(matches_target(path, triple), Ok(false))
}

fn matches_target(path: &Path, triple: &str) -> Result<bool> {
    let bytes = fs::read(path)?;
    let file = object::File::parse(&*bytes)
        .map_err(|e| anyhow::anyhow!("cannot inspect native addon {}: {e}", path.display()))?;
    let architecture = if triple.starts_with("aarch64-") {
        object::Architecture::Aarch64
    } else if triple.starts_with("x86_64-") {
        object::Architecture::X86_64
    } else {
        return Ok(false);
    };
    let format = if triple.contains("-windows-") {
        object::BinaryFormat::Pe
    } else if triple.contains("-apple-") {
        object::BinaryFormat::MachO
    } else {
        object::BinaryFormat::Elf
    };
    // Platform/libc tags belong to the package, not arbitrary directories
    // above it (for example a glibc project checked out under /tmp/musl-tests).
    let root = nearest_package_root(path);
    let relative = root
        .as_ref()
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    let package_name = root
        .as_ref()
        .filter(|root| path_is_inside_node_modules(root))
        .and_then(|root| root.file_name())
        .unwrap_or_default()
        .to_string_lossy();
    let name = format!("{package_name}/{}", relative.to_string_lossy());
    // ELF architecture alone cannot distinguish prebuilt glibc/musl variants.
    let wrong_libc = triple.contains("-linux-")
        && if triple.ends_with("musl") {
            name.contains("glibc") || name.contains("-gnu")
        } else {
            name.contains("musl")
        };
    Ok(file.architecture() == architecture && file.format() == format && !wrong_libc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addon(path: &Path, arm: bool, symbol: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = super::super::binary_tests::macho_import(symbol, 0xfe, true, 1);
        if arm {
            bytes[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
        }
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn discovery_requires_reached_approved_package_and_matches_target() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let package = root.join("node_modules/demo");
        addon(
            &package.join("prebuilds/darwin-arm64/addon.node"),
            true,
            "_napi_create_int32",
        );
        addon(
            &package.join("prebuilds/darwin-x64/addon.node"),
            false,
            "_napi_create_int32",
        );
        fs::write(
            package.join("package.json"),
            r#"{"name":"demo","version":"1"}"#,
        )
        .unwrap();
        let mut ctx = CompilationContext::new(root);
        ctx.native_addon_packages.insert("demo".into());
        collect_declared_addons(&mut ctx, Some("macos")).unwrap();
        assert!(
            ctx.native_addons.is_empty(),
            "unused policy must not enable host"
        );
        ctx.checked_compile_package_native_addon_roots
            .insert(package.clone());
        collect_declared_addons(&mut ctx, Some("macos")).unwrap();
        assert_eq!(ctx.native_addons.len(), 1);
        assert!(ctx
            .native_addons
            .contains_key("demo/prebuilds/darwin-arm64/addon.node"));
        addon(&package.join("bad.node"), true, "_uv_loop_init");
        assert!(collect_declared_addons(&mut ctx, Some("macos"))
            .unwrap_err()
            .to_string()
            .contains("unsupported symbol"));
    }

    #[test]
    fn other_target_payload_edge_is_left_to_runtime() {
        // A napi-rs loader names every platform's binary; the x64 one is not
        // part of an aarch64 graph, approved or not, and records nothing.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let payload = root.join("node_modules/demo-darwin-x64");
        addon(&payload.join("demo.node"), false, "_napi_create_int32");
        fs::write(
            payload.join("package.json"),
            r#"{"name":"demo-darwin-x64","version":"1"}"#,
        )
        .unwrap();
        let mut ctx = CompilationContext::new(root.clone());
        let binary = payload.join("demo.node");
        assert!(
            super::super::collect_or_refuse_node_addon(&mut ctx, &binary, Some("macos")).unwrap()
        );
        assert!(ctx.native_addons.is_empty());
        // A binary for the target itself is still subject to the policy.
        let native = root.join("node_modules/demo-darwin-arm64");
        addon(&native.join("demo.node"), true, "_napi_create_int32");
        fs::write(
            native.join("package.json"),
            r#"{"name":"demo-darwin-arm64","version":"1"}"#,
        )
        .unwrap();
        let binary = native.join("demo.node");
        assert!(
            super::super::collect_or_refuse_node_addon(&mut ctx, &binary, Some("macos"))
                .unwrap_err()
                .to_string()
                .contains("perry.nativeAddons")
        );
    }

    #[cfg(unix)]
    #[test]
    fn isolated_store_payload_inherits_the_wrapper_opt_in() {
        // pnpm's global virtual store: the wrapper and each payload live in
        // separate store directories; only the wrapper's own `node_modules`
        // links the payload, and the project links the wrapper.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let store = root.join("store");
        let wrapper = store.join("demo/1/node_modules/@demo/addon");
        let payload = store.join("demo-darwin-arm64/1/node_modules/@demo/addon-darwin-arm64");
        fs::create_dir_all(&wrapper).unwrap();
        addon(&payload.join("addon.node"), true, "_napi_create_int32");
        fs::write(
            wrapper.join("package.json"),
            r#"{"name":"@demo/addon","optionalDependencies":{"@demo/addon-darwin-arm64":"1"}}"#,
        )
        .unwrap();
        fs::write(
            payload.join("package.json"),
            r#"{"name":"@demo/addon-darwin-arm64","version":"1"}"#,
        )
        .unwrap();
        std::os::unix::fs::symlink(&payload, wrapper.with_file_name("addon-darwin-arm64")).unwrap();
        fs::write(wrapper.join("index.js"), "").unwrap();

        let mut ctx = CompilationContext::new(root.join("app"));
        ctx.native_addon_packages.insert("@demo/addon".into());
        let binary = payload.join("addon.node");
        // Unwalked, the wrapper grants nothing.
        assert!(
            super::super::collect_or_refuse_node_addon(&mut ctx, &binary, Some("macos"))
                .unwrap_err()
                .to_string()
                .contains("perry.nativeAddons")
        );
        // The walk enters the wrapper's loader, then reaches the payload.
        super::super::note_native_addon_wrapper(&mut ctx, &wrapper.join("index.js"));
        assert!(
            super::super::collect_or_refuse_node_addon(&mut ctx, &binary, Some("macos")).unwrap()
        );
        let record = &ctx.native_addons["@demo/addon-darwin-arm64/addon.node"];
        assert_eq!(record.package, "@demo/addon");
    }

    #[test]
    fn declared_project_paths_are_exact_and_target_checked() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::write(root.join("package.json"), "{}").unwrap();
        let path = root.join("native/addon.node");
        addon(&path, true, "_napi_create_int32");
        addon(&root.join("native/unlisted.node"), true, "_uv_loop_init");
        let mut ctx = CompilationContext::new(root);
        ctx.native_addon_paths
            .insert(path, "native/addon.node".into());
        collect_declared_addons(&mut ctx, Some("macos")).unwrap();
        assert_eq!(ctx.native_addons.len(), 1);
        assert!(ctx.native_addons.contains_key("$project/native/addon.node"));
        assert!(collect_declared_addons(&mut ctx, Some("linux"))
            .unwrap_err()
            .to_string()
            .contains("does not match target"));
    }
}
