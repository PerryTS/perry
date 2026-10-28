use super::*;

/// Issue #2531 — a symlinked `perry` (the `cargo`/dev-install default,
/// e.g. `~/.cargo/bin/perry -> .../target/release/perry`) must still
/// locate the workspace root. Without canonicalizing the exe first, the
/// `../../` walk climbs the symlink's own ancestors and misses the
/// workspace, silently dropping the per-feature `perry-ext-*` libs.
#[cfg(unix)]
mod workspace_root_symlink_tests {
    use super::super::workspace_root_from_exe;

    #[test]
    fn symlinked_exe_resolves_to_workspace_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // Lay out a fake workspace: <root>/target/release/perry plus the
        // two marker crates the detector keys off of.
        let real_dir = root.join("target/release");
        std::fs::create_dir_all(&real_dir).expect("mkdir target/release");
        let real_exe = real_dir.join("perry");
        std::fs::write(&real_exe, b"#!/bin/true\n").expect("write exe");
        std::fs::create_dir_all(root.join("crates/perry-runtime")).expect("mkdir runtime");
        std::fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).expect("mkdir gh");

        // Install perry as a symlink under a sibling "bin" dir whose
        // ancestors contain no workspace markers (mimics ~/.cargo/bin).
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
        let link = bin_dir.join("perry");
        std::os::unix::fs::symlink(&real_exe, &link).expect("symlink");

        let found = workspace_root_from_exe(&link).expect("workspace root via symlink");
        assert_eq!(
            found,
            std::fs::canonicalize(root).expect("canonicalize root")
        );
    }

    #[test]
    fn missing_ancestor_does_not_abort_before_match() {
        // The real binary lives at <root>/target/release/perry; the
        // deeper `../../../..` ancestor may climb above the tempdir but
        // must not bail before the `../..` match at the workspace root.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let real_dir = root.join("target/release");
        std::fs::create_dir_all(&real_dir).expect("mkdir target/release");
        let real_exe = real_dir.join("perry");
        std::fs::write(&real_exe, b"#!/bin/true\n").expect("write exe");
        std::fs::create_dir_all(root.join("crates/perry-runtime")).expect("mkdir runtime");
        std::fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).expect("mkdir gh");

        let found = workspace_root_from_exe(&real_exe).expect("workspace root");
        assert_eq!(
            found,
            std::fs::canonicalize(root).expect("canonicalize root")
        );
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
#[path = "tests/manifest_parse_tests.rs"]
mod manifest_parse_tests;

#[cfg(test)]
mod lexical_path_tests {
    use super::*;

    #[test]
    fn lexical_normalization_preserves_leading_parent_segments() {
        assert_eq!(
            normalize_path_lexically(std::path::Path::new("../../dep")),
            std::path::PathBuf::from("../../dep")
        );
    }

    #[test]
    fn lexical_normalization_pops_only_normal_segments() {
        assert_eq!(
            normalize_path_lexically(std::path::Path::new("app/../../dep")),
            std::path::PathBuf::from("../dep")
        );
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

#[cfg(test)]
mod declaration_sidecar_tests {
    use super::*;
    use std::collections::HashSet;

    mod compile_package;

    fn write_typed_js_package(root: &Path, package_name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let package_dir = root.join("node_modules").join(package_name);
        std::fs::create_dir_all(package_dir.join("dist")).expect("mkdir package dist");
        let implementation = package_dir.join("dist/index.js");
        let declaration = package_dir.join("dist/index.d.ts");
        std::fs::write(&implementation, "export class Codex {}\n").expect("write js");
        std::fs::write(&declaration, "export declare class Codex {}\n").expect("write dts");
        std::fs::write(
            package_dir.join("package.json"),
            serde_json::json!({
                "name": package_name,
                "type": "module",
                "module": "./dist/index.js",
                "types": "./dist/index.d.ts",
                "exports": {
                    ".": {
                        "types": "./dist/index.d.ts",
                        "import": "./dist/index.js"
                    }
                }
            })
            .to_string(),
        )
        .expect("write package.json");
        (package_dir, implementation, declaration)
    }

    #[test]
    fn package_exports_types_resolve_as_declaration_sidecar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (package_dir, implementation, declaration) =
            write_typed_js_package(dir.path(), "typed-js");

        let found = resolve_package_declaration_entry(&package_dir, None, Some(&implementation))
            .expect("declaration sidecar");

        assert_eq!(
            found,
            declaration.canonicalize().expect("canonical declaration")
        );
    }

    #[test]
    fn typed_node_modules_js_stays_interpreted_without_compile_package_opt_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let (_package_dir, implementation, declaration) = write_typed_js_package(root, "typed-js");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).expect("mkdir src");
        let importer = src_dir.join("main.ts");
        std::fs::write(&importer, "import { Codex } from 'typed-js';\n").expect("write importer");

        let resolved = resolve_import(
            "typed-js",
            &importer,
            root,
            &HashSet::new(),
            &BTreeSet::new(),
        )
        .expect("resolve typed-js");

        assert_eq!(resolved.1, ModuleKind::Interpreted);
        assert_eq!(
            resolved.0,
            implementation
                .canonicalize()
                .expect("canonical implementation")
        );
        assert_eq!(
            declaration_sidecar_for_resolved_import("typed-js", &resolved.0).expect("sidecar"),
            declaration.canonicalize().expect("canonical declaration")
        );
    }

    #[test]
    fn typed_node_modules_js_becomes_native_with_compile_package_opt_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let (_package_dir, implementation, declaration) = write_typed_js_package(root, "typed-js");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir).expect("mkdir src");
        let importer = src_dir.join("main.ts");
        std::fs::write(&importer, "import { Codex } from 'typed-js';\n").expect("write importer");

        let compile_packages = HashSet::from(["typed-js".to_string()]);
        let resolved = resolve_import(
            "typed-js",
            &importer,
            root,
            &compile_packages,
            &BTreeSet::new(),
        )
        .expect("resolve typed-js");

        assert_eq!(resolved.1, ModuleKind::NativeCompiled);
        assert_eq!(
            resolved.0,
            implementation
                .canonicalize()
                .expect("canonical implementation")
        );
        assert_eq!(
            declaration_sidecar_for_resolved_import("typed-js", &resolved.0).expect("sidecar"),
            declaration.canonicalize().expect("canonical declaration")
        );
    }

    #[test]
    fn compile_package_membership_uses_path_components() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let path = root
            .join("node_modules")
            .join("left-pad")
            .join("lib")
            .join("index.js");
        let compile_packages = HashSet::from(["left-pad".to_string()]);

        assert!(is_in_compile_package(&path, &compile_packages));
        assert!(!is_in_compile_package(
            &root
                .join("not_node_modules")
                .join("left-pad")
                .join("index.js"),
            &compile_packages
        ));

        // A scoped name sits two levels under `node_modules`; a nested
        // `node_modules` tree names its own packages.
        let scoped = HashSet::from(["@noble/hashes".to_string()]);
        let nested = root
            .join("node_modules")
            .join("@noble")
            .join("curves")
            .join("node_modules")
            .join("@noble")
            .join("hashes")
            .join("src")
            .join("sha256.js");
        assert!(is_in_compile_package(&nested, &scoped));
        assert!(!is_in_compile_package(
            &root
                .join("node_modules")
                .join("@noble")
                .join("curves")
                .join("index.js"),
            &scoped
        ));
        assert!(!is_in_compile_package(
            &root.join("@noble").join("hashes").join("index.js"),
            &scoped
        ));
        assert!(!is_in_compile_package(&path, &HashSet::new()));
    }

    #[test]
    fn declaration_file_detection_includes_mts_and_cts_sidecars() {
        assert!(is_declaration_file(Path::new("index.d.ts")));
        assert!(is_declaration_file(Path::new("index.d.mts")));
        assert!(is_declaration_file(Path::new("index.d.cts")));
        assert!(!is_declaration_file(Path::new("index.ts")));
    }

    /// #3527 (blocker #4): `enumerate_installed_packages` must surface every
    /// package name in the `node_modules` tree — flat deps, `@scope/pkg`
    /// entries, and transitive deps nested under a package's own
    /// `node_modules` — so the `"*"` / `"@scope/*"` wildcard in
    /// `perry.compilePackages` can be expanded into concrete names. npm
    /// bookkeeping dirs (`.bin`, `.cache`) must be skipped.
    #[test]
    fn enumerate_installed_packages_covers_flat_scoped_and_nested() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let nm = root.join("node_modules");

        // Flat deps + npm bookkeeping that must be ignored.
        std::fs::create_dir_all(nm.join("express")).expect("mkdir express");
        std::fs::create_dir_all(nm.join("qs")).expect("mkdir qs");
        std::fs::create_dir_all(nm.join(".bin")).expect("mkdir .bin");
        std::fs::create_dir_all(nm.join(".cache")).expect("mkdir .cache");
        std::fs::write(nm.join(".package-lock.json"), b"{}").expect("write lockfile");

        // Scoped package.
        std::fs::create_dir_all(nm.join("@scope/pkg")).expect("mkdir @scope/pkg");

        // Transitive dep npm chose not to hoist (nested node_modules), plus a
        // nested scoped dep two levels down.
        std::fs::create_dir_all(nm.join("express/node_modules/nested-dep"))
            .expect("mkdir nested-dep");
        std::fs::create_dir_all(nm.join("express/node_modules/@deep/leaf"))
            .expect("mkdir @deep/leaf");

        let found = enumerate_installed_packages(root);

        assert!(found.contains("express"), "flat dep");
        assert!(found.contains("qs"), "flat dep");
        assert!(found.contains("@scope/pkg"), "scoped dep");
        assert!(found.contains("nested-dep"), "nested transitive dep");
        assert!(found.contains("@deep/leaf"), "nested scoped dep");
        assert!(!found.contains(".bin"), "npm bin dir skipped");
        assert!(!found.contains(".cache"), "npm cache dir skipped");
        assert!(
            !found.contains(".package-lock.json"),
            "lockfile (a file, not a dir) skipped"
        );
        assert_eq!(found.len(), 5, "exactly the five real packages");
    }

    /// #12822: pnpm gives each workspace member a `node_modules/@scope`
    /// symlink pointing back at the top-level `node_modules/@scope`, so the
    /// directory tree `@scope/a/node_modules/@scope/a/node_modules/...` is
    /// infinite — and, because every member is reachable at every level,
    /// exponential in the member count. Enumeration must terminate and still
    /// report both members exactly once.
    #[cfg(unix)]
    #[test]
    fn enumerate_installed_packages_terminates_on_pnpm_scope_cycle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let nm = root.join("node_modules");

        for member in ["a", "b"] {
            std::fs::create_dir_all(root.join("packages").join(member).join("node_modules"))
                .expect("mkdir member");
        }
        std::fs::create_dir_all(nm.join("@scope")).expect("mkdir scope");

        // The top-level scope dir links out to the workspace members...
        for member in ["a", "b"] {
            std::os::unix::fs::symlink(
                root.join("packages").join(member),
                nm.join("@scope").join(member),
            )
            .expect("symlink member");
            // ...and each member links the scope dir back to the top level.
            std::os::unix::fs::symlink(
                nm.join("@scope"),
                root.join("packages")
                    .join(member)
                    .join("node_modules")
                    .join("@scope"),
            )
            .expect("symlink scope back-pointer");
        }

        let found = enumerate_installed_packages(root);

        assert!(found.contains("@scope/a"), "first workspace member");
        assert!(found.contains("@scope/b"), "second workspace member");
        assert_eq!(found.len(), 2, "exactly the two members, each once");
    }
}

/// Issue #5039 — Node subpath imports (`#…` specifiers resolved through the
/// importing package's own `package.json` `"imports"` map). chalk 5 loads
/// its vendored deps this way (`import ansiStyles from '#ansi-styles'`);
/// before the fix the specifier fell through bare-package resolution, the
/// import silently came up empty, and every chalk style table was blank
/// (ink's `<Text dimColor>` rendered `[object Object]`).
mod subpath_imports_tests {
    use super::super::{resolve_import, ModuleKind};
    use std::collections::{BTreeSet, HashSet};
    use std::path::PathBuf;

    fn write_chalk_like_package(root: &std::path::Path) -> PathBuf {
        let pkg = root.join("node_modules/chalky");
        std::fs::create_dir_all(pkg.join("source/vendor/ansi-styles")).unwrap();
        std::fs::create_dir_all(pkg.join("source/vendor/supports-color")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r##"{
                "name": "chalky",
                "type": "module",
                "exports": "./source/index.js",
                "imports": {
                    "#ansi-styles": "./source/vendor/ansi-styles/index.js",
                    "#supports-color": {
                        "node": "./source/vendor/supports-color/index.js",
                        "default": "./source/vendor/supports-color/browser.js"
                    }
                }
            }"##,
        )
        .unwrap();
        std::fs::write(pkg.join("source/index.js"), "export default 1;\n").unwrap();
        std::fs::write(
            pkg.join("source/vendor/ansi-styles/index.js"),
            "export default {};\n",
        )
        .unwrap();
        std::fs::write(
            pkg.join("source/vendor/supports-color/index.js"),
            "export default { stdout: false };\n",
        )
        .unwrap();
        std::fs::write(
            pkg.join("source/vendor/supports-color/browser.js"),
            "export default { stdout: false };\n",
        )
        .unwrap();
        pkg
    }

    #[test]
    fn hash_specifier_resolves_through_imports_map() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg = write_chalk_like_package(root);
        let importer = pkg.join("source/index.js");

        let compile_packages: HashSet<String> = ["chalky".to_string()].into_iter().collect();
        let compile_package_dirs: BTreeSet<PathBuf> = BTreeSet::new();

        let (resolved, kind) = resolve_import(
            "#ansi-styles",
            &importer,
            root,
            &compile_packages,
            &compile_package_dirs,
        )
        .expect("#ansi-styles must resolve via the imports map");
        assert_eq!(
            resolved,
            pkg.join("source/vendor/ansi-styles/index.js")
                .canonicalize()
                .unwrap()
        );
        // The mapped file is inside a compile package → compiled natively.
        assert_eq!(kind, ModuleKind::NativeCompiled);
    }

    #[test]
    fn conditional_imports_target_prefers_node_over_default() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg = write_chalk_like_package(root);
        let importer = pkg.join("source/index.js");

        let compile_packages: HashSet<String> = ["chalky".to_string()].into_iter().collect();
        let compile_package_dirs: BTreeSet<PathBuf> = BTreeSet::new();

        let (resolved, _) = resolve_import(
            "#supports-color",
            &importer,
            root,
            &compile_packages,
            &compile_package_dirs,
        )
        .expect("#supports-color must resolve via the imports map");
        assert_eq!(
            resolved,
            pkg.join("source/vendor/supports-color/index.js")
                .canonicalize()
                .unwrap(),
            "conditional imports entry must pick `node`, not the browser `default`"
        );
    }

    #[test]
    fn unmapped_hash_specifier_stays_unresolved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pkg = write_chalk_like_package(root);
        let importer = pkg.join("source/index.js");

        assert!(
            resolve_import("#nope", &importer, root, &HashSet::new(), &BTreeSet::new(),).is_none(),
            "a `#` specifier missing from the imports map must not resolve"
        );
    }
}

#[cfg(test)]
mod exports_candidates_tests {
    use crate::commands::compile::resolve::resolve_exports_candidates;

    #[test]
    fn pruned_import_target_falls_back_to_default() {
        // @swc/helpers shape under Next.js standalone output: file tracing
        // prunes esm/, so the `import` condition target is absent on disk and
        // the resolver must surface `default` (cjs) as a later candidate.
        let exports: serde_json::Value = serde_json::json!({
            ".": { "import": "./esm/index.js", "default": "./cjs/index.cjs" },
            "./_/_interop_require_default": {
                "import": "./esm/_interop_require_default.js",
                "default": "./cjs/_interop_require_default.cjs"
            }
        });
        let candidates = resolve_exports_candidates(&exports, "./_/_interop_require_default");
        assert_eq!(
            candidates,
            vec![
                "./esm/_interop_require_default.js".to_string(),
                "./cjs/_interop_require_default.cjs".to_string(),
            ]
        );
    }

    #[test]
    fn wildcard_candidates_expand_star() {
        let exports: serde_json::Value = serde_json::json!({
            "./cjs/*": { "import": "./esm/*.js", "default": "./cjs/*.cjs" }
        });
        let candidates = resolve_exports_candidates(&exports, "./cjs/foo");
        assert_eq!(
            candidates,
            vec!["./esm/foo.js".to_string(), "./cjs/foo.cjs".to_string()]
        );
    }

    #[test]
    fn wildcard_candidates_reject_overlapping_bounds() {
        let exports: serde_json::Value = serde_json::json!({
            "./long*long": "./wrong/*.js"
        });
        assert!(resolve_exports_candidates(&exports, "./long").is_empty());
    }

    #[test]
    fn wildcard_candidates_prefer_the_most_specific_pattern() {
        let exports: serde_json::Value = serde_json::json!({
            "./*": "./broad/*.js",
            "./features/*": "./specific/*.js"
        });
        let candidates = resolve_exports_candidates(&exports, "./features/a");
        assert_eq!(
            candidates,
            vec![
                "./specific/a.js".to_string(),
                "./broad/features/a.js".to_string()
            ]
        );
    }

    /// #5237 — a Node "exports" *fallback array* (an ordered list of targets,
    /// here a conditions-object followed by a plain string) must expand to its
    /// inner targets. Before the fix the array form produced no candidates at
    /// all, so the resolver fell through to the legacy `"module"` field.
    #[test]
    fn array_form_candidates_expand() {
        let exports: serde_json::Value = serde_json::json!({
            ".": [
                { "import": "./index.mjs", "require": "./build/index.cjs" },
                "./build/index.cjs"
            ]
        });
        let candidates = resolve_exports_candidates(&exports, ".");
        assert_eq!(
            candidates,
            vec!["./index.mjs".to_string(), "./build/index.cjs".to_string()]
        );
    }
}

/// #5237 — `"exports"`, when it covers the requested specifier, takes
/// precedence over the legacy `"module"`/`"main"` fields (matching Node's
/// resolution algorithm). The live regression was `y18n` (a `yargs` dep):
/// perry picked its `"module": "./build/lib/index.js"` (a named-export-only
/// file with no `default`) instead of the `exports.import` target
/// `./index.mjs`, breaking `import y18n from "y18n"`.
mod exports_over_module_precedence_tests {
    use super::super::resolve_package_entry;

    /// Lay out a package mirroring the #5237 minimal repro: `module` points at
    /// a built `build/lib/index.js`, but an array-form `exports` points its
    /// `import` condition at `./index.mjs`.
    fn write_y18n_like_package(root: &std::path::Path) -> std::path::PathBuf {
        let pkg = root.join("node_modules/y18n-like");
        std::fs::create_dir_all(pkg.join("build/lib")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{
                "name": "y18n-like",
                "version": "1.0.0",
                "type": "module",
                "main": "./build/index.cjs",
                "module": "./build/lib/index.js",
                "exports": {
                    ".": [
                        { "import": "./index.mjs", "require": "./build/index.cjs" },
                        "./build/index.cjs"
                    ]
                }
            }"#,
        )
        .unwrap();
        std::fs::write(pkg.join("index.mjs"), "export default 1;\n").unwrap();
        std::fs::write(pkg.join("build/lib/index.js"), "export function x(){}\n").unwrap();
        std::fs::write(pkg.join("build/index.cjs"), "module.exports = 1;\n").unwrap();
        pkg
    }

    #[test]
    fn exports_import_wins_over_module_field() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = write_y18n_like_package(dir.path());

        let resolved = resolve_package_entry(&pkg, None).expect("resolve entry");
        assert_eq!(
            resolved,
            pkg.join("index.mjs"),
            "exports.import (./index.mjs) must win over module (./build/lib/index.js)"
        );
    }

    /// Regression: a package with `module` but NO `exports` still resolves via
    /// the `module` field (and `main`), exactly as before.
    #[test]
    fn module_field_still_resolves_without_exports() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("node_modules/legacy-mod");
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{
                "name": "legacy-mod",
                "type": "module",
                "main": "./dist/index.cjs",
                "module": "./dist/index.mjs"
            }"#,
        )
        .unwrap();
        std::fs::write(pkg.join("dist/index.mjs"), "export default 1;\n").unwrap();
        std::fs::write(pkg.join("dist/index.cjs"), "module.exports = 1;\n").unwrap();

        let resolved = resolve_package_entry(&pkg, None).expect("resolve entry");
        assert_eq!(resolved, pkg.join("dist/index.mjs"));
    }

    /// Regression: a normal object-form `exports`-only package (no `module`)
    /// resolves through `exports` unchanged.
    #[test]
    fn plain_object_exports_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let pkg = dir.path().join("node_modules/modern-pkg");
        std::fs::create_dir_all(pkg.join("dist")).unwrap();
        std::fs::write(
            pkg.join("package.json"),
            r#"{
                "name": "modern-pkg",
                "type": "module",
                "exports": { ".": { "import": "./dist/index.js" } }
            }"#,
        )
        .unwrap();
        std::fs::write(pkg.join("dist/index.js"), "export default 1;\n").unwrap();

        let resolved = resolve_package_entry(&pkg, None).expect("resolve entry");
        assert_eq!(resolved, pkg.join("dist/index.js"));
    }
}

/// pnpm layout regression — a package's own `node_modules` usually holds
/// only `.bin`, while its real dependencies live in the
/// `.pnpm/<pkg>@<v>/node_modules` sibling one level further up. The
/// resolver must consult EVERY ancestor `node_modules` (Node's algorithm),
/// not stop at the first one found.
mod ancestor_node_modules_tests {
    use super::super::ancestor_node_modules_dirs;

    #[test]
    fn walk_lists_every_ancestor_node_modules_nearest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();

        // <root>/node_modules/.pnpm/fxp@1/node_modules/fxp/node_modules/.bin
        //                                             └── strnum lives in the
        //                                                 .pnpm sibling dir
        let pnpm_nm = root.join("node_modules/.pnpm/fxp@1/node_modules");
        let pkg_local_nm = pnpm_nm.join("fxp/node_modules");
        std::fs::create_dir_all(pkg_local_nm.join(".bin")).expect("mkdir .bin");
        std::fs::create_dir_all(pnpm_nm.join("strnum")).expect("mkdir strnum");
        let start = pnpm_nm.join("fxp/src/xmlparser");
        std::fs::create_dir_all(&start).expect("mkdir src");

        // Ancestors of the temp dir itself (e.g. /tmp) are outside the
        // fixture; keep only the chain under `root` so the exact-order
        // assertion cannot flake on a host machine's own node_modules.
        let dirs: Vec<_> = ancestor_node_modules_dirs(&start)
            .into_iter()
            .filter(|d| d.starts_with(root))
            .collect();
        assert_eq!(
            dirs,
            vec![pkg_local_nm, pnpm_nm, root.join("node_modules")],
            "every ancestor node_modules, nearest first: the package-local `.bin` \
             host, then the .pnpm sibling hosting the real deps, then the project root"
        );
    }
}

#[cfg(test)]
#[path = "tests/bun_export_condition_tests.rs"]
mod bun_export_condition_tests;
