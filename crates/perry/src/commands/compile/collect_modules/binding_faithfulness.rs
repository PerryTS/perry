use std::path::Path;

use anyhow::Result;

use super::super::CompilationContext;

/// Report when a bundled partial binding shadows an installed package, or
/// reject that choice under the strict faithfulness policy.
pub(super) fn audit_native_binding_choice(
    source: &str,
    entry_path: &Path,
    importer: &Path,
    ctx: &mut CompilationContext,
) -> Result<()> {
    if source.starts_with('.') || source.starts_with('/') {
        return Ok(());
    }

    let (package_name, binding) = lookup_well_known_for_import(source);
    let Some(binding) = binding else {
        return Ok(());
    };
    // Node builtins have no meaningful npm copy to shadow.
    if binding.node_builtin
        || binding.is_faithful()
        || !node_modules_copy_on_disk(&package_name, entry_path, importer)
    {
        return Ok(());
    }

    if faithful_bindings_required() {
        anyhow::bail!(
            "`{pkg}` resolves to the bundled native binding `{krate}`, which is \
             a PARTIAL drop-in (not the full npm API), but a `node_modules/{pkg}` \
             copy is installed and `PERRY_REQUIRE_FAITHFUL_BINDINGS=1` is set. \
             Refusing to auto-prefer the partial binding.\n\
             \n\
             Either add `{pkg}` to `perry.compilePackages` (+ \
             `perry.allow.compilePackages`) to AOT-compile the real JavaScript, \
             or unset PERRY_REQUIRE_FAITHFUL_BINDINGS to accept the partial \
             binding.\n\
             \n\
             (imported from {importer})",
            pkg = package_name,
            krate = binding.krate,
            importer = importer.display(),
        );
    }
    ctx.partial_binding_autoprefers.insert(package_name);
    Ok(())
}

/// Preserve a registered subpath such as `mysql2/promise` before falling back
/// to the root-package binding. The root name remains the on-disk/config key.
fn lookup_well_known_for_import(
    source: &str,
) -> (
    String,
    Option<&'static super::super::well_known::WellKnownBinding>,
) {
    let (package_name, _) = super::super::resolve::parse_package_specifier(source);
    let binding = super::super::well_known::lookup_well_known(source)
        .or_else(|| super::super::well_known::lookup_well_known(&package_name));
    (package_name, binding)
}

/// Probe both resolver ancestor chains so nested and pnpm layouts are covered.
fn node_modules_copy_on_disk(pkg_name: &str, entry_path: &Path, importer: &Path) -> bool {
    let mut starts = vec![entry_path];
    if importer != entry_path {
        starts.push(importer);
    }
    starts.into_iter().any(|start| {
        super::super::resolve::ancestor_node_modules_dirs(start)
            .into_iter()
            .any(|node_modules| node_modules.join(pkg_name).is_dir())
    })
}

fn faithful_bindings_required_value(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true"))
}

fn faithful_bindings_required() -> bool {
    faithful_bindings_required_value(
        std::env::var("PERRY_REQUIRE_FAITHFUL_BINDINGS")
            .ok()
            .as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_mode_requires_an_enabled_value() {
        for value in [Some("1"), Some("true"), Some(" TRUE ")] {
            assert!(faithful_bindings_required_value(value), "value: {value:?}");
        }
        for value in [None, Some(""), Some("0"), Some("false"), Some("no")] {
            assert!(!faithful_bindings_required_value(value), "value: {value:?}");
        }
    }

    #[test]
    fn lookup_preserves_registered_subpaths_before_falling_back() {
        let (root, binding) = lookup_well_known_for_import("mysql2/promise");
        assert_eq!(root, "mysql2");
        assert_eq!(binding.expect("subpath binding").package, "mysql2/promise");

        let (root, binding) = lookup_well_known_for_import("dotenv/config");
        assert_eq!(root, "dotenv");
        assert_eq!(binding.expect("root fallback").package, "dotenv");
    }

    #[test]
    fn installed_copy_probe_uses_root_package_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let entry = dir.path().join("src/main.ts");
        let importer = dir.path().join("node_modules/consumer/index.js");
        std::fs::create_dir_all(dir.path().join("node_modules/mysql2")).unwrap();
        std::fs::create_dir_all(importer.parent().unwrap()).unwrap();
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();

        assert!(node_modules_copy_on_disk("mysql2", &entry, &importer));
        assert!(!node_modules_copy_on_disk(
            "mysql2/promise",
            &entry,
            &importer
        ));
    }
}
