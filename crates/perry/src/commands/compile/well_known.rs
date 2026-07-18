//! Well-known native bindings registry (#466 Phase 4).
//!
//! Source-of-truth: `crates/perry/well_known_bindings.toml`,
//! embedded into the binary via `include_str!`. Parsed on first
//! lookup, cached for the process's lifetime.
//!
//! See `docs/src/native-libraries/manifest-v1.md` for the resolution
//! precedence this fits into.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One row of the well-known bindings table — what perry's bundled
/// wrappers expose to programs that import the bare npm name.
#[derive(Debug, Clone)]
pub struct WellKnownBinding {
    /// npm package name as the user writes it (`"dotenv"`,
    /// `"mysql2/promise"`).
    pub package: String,
    /// Workspace crate that ships the staticlib (e.g.
    /// `"perry-ext-dotenv"`).
    pub krate: String,
    /// Library basename Cargo emits — `lib<name>.a`. Usually the
    /// crate name with `-` replaced by `_`, but stated explicitly
    /// in the toml so the lookup is unambiguous.
    pub lib: String,
    /// GitHub issue tracking the migration. Surfaced in error
    /// messages when the bundled `.a` is absent.
    pub tracking: Option<String>,
}

/// Parse the embedded toml on first call; reuse on subsequent ones.
/// Result map is indexed by bare package name.
fn registry() -> &'static BTreeMap<String, WellKnownBinding> {
    static CACHE: OnceLock<BTreeMap<String, WellKnownBinding>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let raw = include_str!("../../../well_known_bindings.toml");
        parse_well_known_toml(raw).unwrap_or_else(|err| {
            // Bundled toml shipping malformed is a build-time bug.
            // Panic loudly so it surfaces in CI rather than at the
            // first user-facing import.
            panic!(
                "well_known_bindings.toml failed to parse — this is a perry \
                 build bug, not a user error: {}",
                err
            )
        })
    })
}

/// Look up `package` in the well-known table. Strips a leading
/// `node:` prefix to match Perry's other resolvers; that prefix is
/// never legal in npm package names anyway, but seeing
/// `import 'node:dotenv'` in user code is harmless under the same
/// rule.
pub fn lookup_well_known(package: &str) -> Option<&'static WellKnownBinding> {
    let normalized = package.strip_prefix("node:").unwrap_or(package);
    registry().get(normalized)
}

/// Walk every binding declared in `well_known_bindings.toml`, in
/// BTreeMap (alphabetical) order. Used by `perry native list`
/// (#466 Phase 3) and any other tooling that needs to enumerate
/// the bundled surface.
pub fn iter_well_known() -> impl Iterator<Item = &'static WellKnownBinding> {
    registry().values()
}

/// Platform-correct static-library filename for an ext-binding lib stem.
///
/// Cargo emits `lib<stem>.a` on Unix-likes but `<stem>.lib` on
/// Windows/MSVC. `target_triple` is the rust triple being built for
/// (`None` = host build → use the host OS). Every call site that locates
/// a well-known binding's staticlib must go through this: previously the
/// `lib<stem>.a` name was hardcoded, so on a Windows build the real
/// `<stem>.lib` artifact was looked up under a name that never exists,
/// the binding was silently skipped, and the final link failed with
/// unresolved `js_*` symbols (e.g. perry-ext-ws's `js_ws_*` when a
/// program `import`s `ws`).
pub fn ext_staticlib_filename(lib_stem: &str, target_triple: Option<&str>) -> String {
    let is_windows = match target_triple {
        Some(t) => t.contains("windows"),
        None => cfg!(target_os = "windows"),
    };
    if is_windows {
        format!("{}.lib", lib_stem)
    } else {
        format!("lib{}.a", lib_stem)
    }
}

/// Resolve the bundled staticlib path for `binding`, given the perry
/// workspace root (from `find_perry_workspace_root`) and an optional
/// rust target triple. When `target_triple` is `Some`, look in the
/// per-target output dir (`target/<triple>/release/`); otherwise the
/// host build dir (`target/release/`). Returns `None` when the file
/// isn't present — caller decides whether to error or fall through.
pub fn bundled_staticlib_path_for_target(
    workspace_root: &Path,
    binding: &WellKnownBinding,
    target_triple: Option<&str>,
) -> Option<PathBuf> {
    let release_dir = if let Some(triple) = target_triple {
        workspace_root.join("target").join(triple).join("release")
    } else {
        workspace_root.join("target").join("release")
    };
    let path = release_dir.join(ext_staticlib_filename(&binding.lib, target_triple));
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

fn parse_well_known_toml(raw: &str) -> Result<BTreeMap<String, WellKnownBinding>, String> {
    // Hand-written parser keeps the dep surface small and avoids
    // pulling another toml-deserializer alternative — `toml`
    // crate is already in the link surface (used by perry's
    // `package.json` discovery elsewhere). Accept the format we
    // ship; refuse anything else loudly.
    // toml 1.x: `<Value as FromStr>` is now an inline-value parser
    // (e.g. `"foo"` / `42` / `{ k = "v" }`), not a document parser
    // — so `raw.parse::<toml::Value>()` rejects the file's leading
    // comment with "unexpected content, expected nothing". The
    // crate-level `toml::from_str` still runs the document parser
    // and returns a `Value::Table`, which is the shape this code
    // already expects to walk.
    let parsed: toml::Value = toml::from_str(raw).map_err(|e: toml::de::Error| e.to_string())?;

    let bindings_table = parsed
        .get("bindings")
        .and_then(|v| v.as_table())
        .ok_or_else(|| "missing top-level [bindings] table".to_string())?;

    let mut out = BTreeMap::new();
    for (pkg_name, value) in bindings_table {
        let entry_table = value
            .as_table()
            .ok_or_else(|| format!("entry [bindings.{}] is not a table", pkg_name))?;

        let krate = entry_table
            .get("crate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("[bindings.{}] missing required `crate` field", pkg_name))?
            .to_string();

        let lib = entry_table
            .get("lib")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("[bindings.{}] missing required `lib` field", pkg_name))?
            .to_string();

        let tracking = entry_table
            .get("tracking")
            .and_then(|v| v.as_str())
            .map(String::from);

        out.insert(
            pkg_name.clone(),
            WellKnownBinding {
                package: pkg_name.clone(),
                krate,
                lib,
                tracking,
            },
        );
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_toml_parses() {
        // The OnceLock will panic in `registry()` if parsing fails —
        // this test exercises that path explicitly so a malformed
        // shipped toml surfaces in `cargo test` rather than the first
        // user invocation.
        let _ = registry();
    }

    #[test]
    fn dotenv_is_registered() {
        let binding = lookup_well_known("dotenv").expect("dotenv must be a well-known binding");
        assert_eq!(binding.krate, "perry-ext-dotenv");
        assert_eq!(binding.lib, "perry_ext_dotenv");
    }

    #[test]
    fn node_prefix_stripped_on_lookup() {
        let bare = lookup_well_known("dotenv");
        let prefixed = lookup_well_known("node:dotenv");
        assert!(bare.is_some());
        assert!(prefixed.is_some());
    }

    #[test]
    fn unknown_package_returns_none() {
        assert!(lookup_well_known("definitely-not-a-real-package").is_none());
    }

    #[test]
    fn parser_rejects_missing_crate_field() {
        let raw = r#"
            [bindings.foo]
            lib = "foo"
        "#;
        let err = parse_well_known_toml(raw).expect_err("missing crate must reject");
        assert!(err.contains("crate"), "got: {}", err);
        assert!(err.contains("foo"), "got: {}", err);
    }

    /// #466 Phase 4 acceptance: "Each well-known entry validated at
    /// perry startup (errors at install time, not user-import time,
    /// if a bundled crate is missing)". Realized as a CI test here —
    /// every entry in the toml must reference a crate that actually
    /// exists in the workspace, so a release tarball can never ship
    /// a dangling well-known reference.
    #[test]
    fn every_entry_references_a_workspace_crate() {
        // Walk up from `crates/perry/` to the workspace root.
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace
            .expect("workspace root reachable from CARGO_MANIFEST_DIR");

        for binding in iter_well_known() {
            let crate_dir = workspace_root.join("crates").join(&binding.krate);
            assert!(
                crate_dir.is_dir(),
                "well-known binding for `{}` references crate `{}` at `{}` but that directory does not exist. \
                 Either add the crate to the workspace or remove the entry from well_known_bindings.toml.",
                binding.package,
                binding.krate,
                crate_dir.display()
            );
            let crate_cargo = crate_dir.join("Cargo.toml");
            assert!(
                crate_cargo.is_file(),
                "well-known binding for `{}` references crate `{}` but `{}` is missing.",
                binding.package,
                binding.krate,
                crate_cargo.display()
            );
        }
    }

    /// #6303 / #6314 — every `perry-ext-*` crate that depends on perry-runtime
    /// must build it with BOTH the `default` and `stdlib` features.
    ///
    /// These crates are `crate-type = ["staticlib", ...]`, so `libperry_ext_*.a`
    /// physically bundles a copy of perry-runtime, and perry links the ext
    /// archives BEFORE stdlib (`prefer_well_known_before_stdlib`) — the bundled
    /// copy wins the link for every symbol it exports. The workspace dep is
    /// `default-features = false`, and a per-crate `cargo build -p perry-ext-<x>`
    /// (what release-packages.yml does in its per-crate loop) is what makes the
    /// divergence real.
    ///
    /// * `default` (#6303) keeps the bundled copy feature-identical to the
    ///   shipped runtime, so unconditionally-exported, feature-gated dispatchers
    ///   (`js_string_replace_search_dyn`, `js_native_call_method`, …) don't
    ///   silently degrade — e.g. `str.replace(re, fn)` keeps firing its callback.
    /// * `stdlib` (#6314) gates OUT perry-runtime's no-op `stdlib_stubs` module
    ///   (`js_stdlib_init_dispatch`, `js_stdlib_process_pending`, the fetch/ws/
    ///   readline no-ops). Without it the bundled no-op wins the link over
    ///   perry-stdlib's real dispatch, so a `node:http` server never registers
    ///   its tokio reactor and dies on the first accept. The link-time strip that
    ///   should drop those members silently no-ops when perry can't find LLVM
    ///   `nm`/`objcopy` (e.g. a stock macOS host), so the copy must not emit them.
    ///
    /// Lives here as a unit test (not an integration test under `tests/`) so it
    /// runs on every PR's `cargo test`.
    #[test]
    fn ext_crates_bundle_a_full_featured_perry_runtime() {
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let crates_dir = manifest_dir
            .parent() // crates/
            .and_then(|p| p.parent()) // workspace
            .expect("workspace root reachable from CARGO_MANIFEST_DIR")
            .join("crates");

        let mut checked = 0usize;
        let mut missing_default: Vec<String> = Vec::new();
        let mut missing_stdlib: Vec<String> = Vec::new();

        for entry in std::fs::read_dir(&crates_dir)
            .expect("read crates/")
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("perry-ext-") {
                continue;
            }
            let Ok(manifest) = std::fs::read_to_string(entry.path().join("Cargo.toml")) else {
                continue;
            };
            // Only the `perry-runtime = { ... }` / `perry-runtime.workspace` forms
            // appear in these crates, so a line-prefix match needs no toml parser.
            let Some(dep_line) = manifest.lines().map(str::trim).find(|l| {
                l.starts_with("perry-runtime.workspace") || l.starts_with("perry-runtime =")
            }) else {
                // perry-ffi-only crate — cannot bundle a divergent runtime copy.
                continue;
            };
            checked += 1;

            // `perry-runtime.workspace = true` inherits `default-features = false`
            // from the workspace dep and adds nothing back — always a violation.
            let has_features = dep_line.contains("features");
            if !(has_features && dep_line.contains("\"default\"")) {
                missing_default.push(format!("  {name}: {dep_line}"));
            }
            if !(has_features && dep_line.contains("\"stdlib\"")) {
                missing_stdlib.push(format!("  {name}: {dep_line}"));
            }
        }

        assert!(
            checked > 0,
            "found no perry-ext-* crate depending on perry-runtime — did the crate \
             layout change? This guard would silently pass forever."
        );
        assert!(
            missing_default.is_empty(),
            "#6303: these perry-ext-* crates bundle a feature-stripped perry-runtime \
             into their staticlib. They are linked BEFORE stdlib/runtime, so their copy \
             wins the link and silently degrades every unconditionally-exported \
             dispatcher whose body is feature-gated (js_string_replace_search_dyn, ...) \
             — e.g. `str.replace(re, fn)` stops invoking its callback.\n\
             Add \"default\" to the perry-runtime `features` list:\n{}",
            missing_default.join("\n")
        );
        assert!(
            missing_stdlib.is_empty(),
            "#6314: these perry-ext-* crates bundle a perry-runtime that still exports \
             the no-op `stdlib_stubs`. They are linked BEFORE stdlib, so the bundled \
             no-op `js_stdlib_init_dispatch` wins the link and perry-stdlib's real \
             dispatch never runs — a node:http server never registers its tokio reactor \
             and dies on the first accept. The link-time strip that should drop those \
             members silently no-ops when perry can't find LLVM nm/objcopy (e.g. a stock \
             macOS host), so the copy must not emit the stubs.\n\
             Add \"stdlib\" to the perry-runtime `features` list:\n{}",
            missing_stdlib.join("\n")
        );
    }
}
