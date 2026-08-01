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

/// How faithful a bundled binding is to the npm package's public API.
///
/// This is the safety marker behind "just works" auto-preference (#466
/// follow-up): perry routes a bare `import 'X'` to the bundled
/// `perry-ext-X` wrapper even when a `node_modules/X` copy is on disk
/// (see `is_native_module` + the resolver short-circuit). That is only
/// safe-by-construction when the wrapper is a genuine drop-in. A wrapper
/// that ports a *subset* of the surface (undici's dispatcher-only client,
/// node-forge's PKI-only slice, lru-cache's numeric-only store) can
/// silently diverge from the real package, so it is marked `Partial` and
/// perry surfaces a diagnostic (and, under
/// `PERRY_REQUIRE_FAITHFUL_BINDINGS=1`, refuses to auto-prefer it).
///
/// Conservative default: a binding with no `compat` field is treated as
/// `Partial`. A wrapper opts IN to `Full` only once it is audited as a
/// complete drop-in for the package's public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BindingCompat {
    /// Audited complete drop-in for the npm package's public API.
    Full,
    /// Ports a subset of the surface (or not yet audited). Not
    /// auto-preferred under a strict-faithful build; a note is emitted
    /// otherwise.
    #[default]
    Partial,
}

impl BindingCompat {
    /// Parse the toml `compat = "..."` value. Unknown / absent → the
    /// conservative `Partial` default.
    fn from_toml(raw: Option<&str>) -> Self {
        match raw {
            Some("full") => BindingCompat::Full,
            // Any other value (including "partial", or a typo) stays
            // conservative — a binding is never faithful by accident.
            _ => BindingCompat::Partial,
        }
    }
}

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
    /// Upstream provenance pin — which release of the npm package this
    /// wrapper ports, and when it was last reviewed against it. See
    /// `docs/src/native-libraries/upstream-pins.md` and the lock-step
    /// gate in `scripts/binding_pins.mjs`. `None` for entries exempt
    /// from pinning (`node_builtin`, `alias_of`, and perry-owned
    /// packages).
    // Provenance metadata parsed from `well_known_bindings.toml` and consulted
    // by the lock-step review gate + unit tests, not by the compile/link path,
    // so these carry no reader in the `perry` bins build and would trip
    // `-D dead-code` without an explicit allow.
    #[allow(dead_code)]
    pub upstream: Option<UpstreamPin>,
    /// `true` when this binding ports a Node.js **builtin** module
    /// (`node:zlib`/`net`/`http`/…) rather than a third-party npm
    /// package. Its upstream is Node core, not an npm dist, so it
    /// carries no npm provenance pin.
    #[allow(dead_code)]
    pub node_builtin: bool,
    /// When set, this row is an **alias** for another binding (a
    /// package subpath like `mysql2/promise`, or a bare-name alias like
    /// `fetch` → `node-fetch`) and shares that binding's provenance
    /// instead of carrying its own pin.
    #[allow(dead_code)]
    pub alias_of: Option<String>,
    /// How faithful this wrapper is to the npm package's public API.
    /// Governs whether auto-preference over an on-disk `node_modules`
    /// copy is safe-by-construction. Absent in the toml → `Partial`.
    pub compat: BindingCompat,
}

impl WellKnownBinding {
    /// Whether this binding is an audited complete drop-in — safe to
    /// auto-prefer over a user's `node_modules` copy without a caveat.
    pub fn is_faithful(&self) -> bool {
        binding_is_faithful(registry(), self)
    }
}

fn binding_is_faithful(
    table: &BTreeMap<String, WellKnownBinding>,
    binding: &WellKnownBinding,
) -> bool {
    let mut current = binding;
    // At most one visit per row: exceeding the table length means aliases
    // contain a cycle. Missing/cyclic targets are never granted faithfulness.
    for _ in 0..=table.len() {
        if let Some(target) = current.alias_of.as_deref() {
            let Some(target_binding) = table.get(target) else {
                return false;
            };
            current = target_binding;
        } else {
            return matches!(current.compat, BindingCompat::Full);
        }
    }
    false
}

/// Provenance pin for a binding's upstream npm package — the same record
/// shape as an upstream reference submodule's `.gitmodules` block
/// (pinned release + content hash + review stamp), carried as toml
/// fields since the upstream here is an npm dist, not a vendored tree.
///
/// The **lock-step rule**: `ported_at` must equal `version`. Re-pinning
/// an upstream release without re-reviewing the wrapper against the
/// upstream diff reds the `binding_pins.mjs --check` gate until
/// `ported_at` advances with the review — an upstream release can never
/// go silently stale, and a pin bump can never outrun its port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamPin {
    /// Pinned upstream npm release (an immutable published version,
    /// e.g. `"17.4.2"`) — the analogue of a release-tag pin.
    pub version: String,
    /// SHA-256 of the npm registry tarball for `version` — the
    /// content hash of record (npm's own `dist.integrity` is sha512;
    /// this is computed from the tarball bytes at pin time so it can
    /// be independently re-verified with `shasum -a 256`).
    pub sha256: String,
    /// Upstream source repository URL, when the package declares one.
    pub repo: Option<String>,
    /// Upstream git commit for the release (`gitHead` from the npm
    /// registry when the publisher recorded it) — empty when unknown.
    pub git_ref: Option<String>,
    /// Release the wrapper was last **reviewed** against. Lock-step:
    /// must equal `version`.
    pub ported_at: String,
    /// ISO date (YYYY-MM-DD) of that review.
    pub date: String,
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

        let node_builtin = entry_table
            .get("node-builtin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let alias_of = entry_table
            .get("alias-of")
            .and_then(|v| v.as_str())
            .map(String::from);

        let compat = BindingCompat::from_toml(entry_table.get("compat").and_then(|v| v.as_str()));

        let upstream = match entry_table.get("upstream") {
            None => None,
            Some(value) => {
                let up = value
                    .as_table()
                    .ok_or_else(|| format!("[bindings.{}.upstream] is not a table", pkg_name))?;
                let required = |field: &str| -> Result<String, String> {
                    up.get(field)
                        .and_then(|v| v.as_str())
                        .map(String::from)
                        .ok_or_else(|| {
                            format!(
                                "[bindings.{}.upstream] missing required `{}` field",
                                pkg_name, field
                            )
                        })
                };
                let version = required("version")?;
                let ported_at = required("ported-at")?;
                // Parse-time lock-step backstop. The authoritative gate is
                // `scripts/binding_pins.mjs --check` (CI); failing here too
                // means a skewed pin can't even ship inside the binary.
                if ported_at != version {
                    return Err(format!(
                        "[bindings.{}.upstream] lock-step violation: ported-at ({}) \
                         != version ({}) — re-review the wrapper against the \
                         upstream diff and advance ported-at with the review",
                        pkg_name, ported_at, version
                    ));
                }
                Some(UpstreamPin {
                    version,
                    sha256: required("sha256")?,
                    repo: up.get("repo").and_then(|v| v.as_str()).map(String::from),
                    git_ref: up.get("ref").and_then(|v| v.as_str()).map(String::from),
                    ported_at,
                    date: required("date")?,
                })
            }
        };

        out.insert(
            pkg_name.clone(),
            WellKnownBinding {
                package: pkg_name.clone(),
                krate,
                lib,
                tracking,
                upstream,
                node_builtin,
                alias_of,
                compat,
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
    fn undici_is_registered() {
        let binding = lookup_well_known("undici").expect("undici must be a well-known binding");
        assert_eq!(binding.krate, "perry-ext-undici");
        assert_eq!(binding.lib, "perry_ext_undici");
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
    fn compat_defaults_to_partial_when_absent() {
        let raw = r#"
            [bindings.foo]
            crate = "perry-ext-foo"
            lib = "perry_ext_foo"
        "#;
        let parsed = parse_well_known_toml(raw).expect("entry parses");
        assert_eq!(parsed["foo"].compat, BindingCompat::Partial);
        assert!(!parsed["foo"].is_faithful());
    }

    #[test]
    fn compat_full_marks_binding_faithful() {
        let raw = r#"
            [bindings.foo]
            crate = "perry-ext-foo"
            lib = "perry_ext_foo"
            compat = "full"
        "#;
        let parsed = parse_well_known_toml(raw).expect("entry parses");
        assert_eq!(parsed["foo"].compat, BindingCompat::Full);
        assert!(parsed["foo"].is_faithful());
    }

    #[test]
    fn compat_unknown_value_stays_conservative() {
        let raw = r#"
            [bindings.foo]
            crate = "perry-ext-foo"
            lib = "perry_ext_foo"
            compat = "mostly"
        "#;
        let parsed = parse_well_known_toml(raw).expect("entry parses");
        // A typo / unrecognized level never grants faithfulness.
        assert_eq!(parsed["foo"].compat, BindingCompat::Partial);
    }

    /// The shipped table's audited posture: documented-subset wrappers
    /// stay `Partial` and are never silently treated as complete drop-ins.
    #[test]
    fn shipped_subset_bindings_are_partial() {
        for name in ["undici", "node-forge", "lru-cache"] {
            let b = lookup_well_known(name).unwrap_or_else(|| panic!("{name} registered"));
            assert_eq!(
                b.compat,
                BindingCompat::Partial,
                "{name} is a documented subset and must stay compat=partial"
            );
        }
    }

    #[test]
    fn shipped_unproven_bindings_are_partial() {
        for name in ["dotenv", "nanoid", "slugify", "uuid"] {
            let b = lookup_well_known(name).unwrap_or_else(|| panic!("{name} registered"));
            assert_eq!(
                b.compat,
                BindingCompat::Partial,
                "{name} omits upstream API/behavior and must stay partial"
            );
        }
    }

    #[test]
    fn aliases_inherit_target_compat_and_cycles_fail_closed() {
        let raw = r#"
            [bindings.full]
            crate = "perry-ext-full"
            lib = "perry_ext_full"
            compat = "full"

            [bindings.alias]
            crate = "perry-ext-full"
            lib = "perry_ext_full"
            alias-of = "full"

            [bindings.a]
            crate = "perry-ext-a"
            lib = "perry_ext_a"
            alias-of = "b"

            [bindings.b]
            crate = "perry-ext-b"
            lib = "perry_ext_b"
            alias-of = "a"
        "#;
        let parsed = parse_well_known_toml(raw).expect("entries parse");
        assert!(binding_is_faithful(&parsed, &parsed["alias"]));
        assert!(!binding_is_faithful(&parsed, &parsed["a"]));
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

    #[test]
    fn upstream_pin_parses() {
        let raw = r#"
            [bindings.foo]
            crate = "perry-ext-foo"
            lib = "perry_ext_foo"

            [bindings.foo.upstream]
            version = "1.2.3"
            sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            repo = "https://github.com/example/foo"
            ref = "0123456789012345678901234567890123456789"
            ported-at = "1.2.3"
            date = "2026-07-29"
        "#;
        let parsed = parse_well_known_toml(raw).expect("pinned entry must parse");
        let pin = parsed["foo"]
            .upstream
            .as_ref()
            .expect("upstream pin present");
        assert_eq!(pin.version, "1.2.3");
        assert_eq!(pin.ported_at, "1.2.3");
        assert_eq!(pin.date, "2026-07-29");
        assert_eq!(pin.repo.as_deref(), Some("https://github.com/example/foo"));
    }

    /// The lock-step rule enforced at parse time: a pin bump
    /// (`version`) that outruns its review (`ported-at`) must refuse
    /// to load — the authoritative CI gate is binding_pins.mjs
    /// --check, but a skewed pin must not even ship inside the binary.
    #[test]
    fn upstream_pin_rejects_lock_step_violation() {
        let raw = r#"
            [bindings.foo]
            crate = "perry-ext-foo"
            lib = "perry_ext_foo"

            [bindings.foo.upstream]
            version = "2.0.0"
            sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            ported-at = "1.2.3"
            date = "2026-07-29"
        "#;
        let err = parse_well_known_toml(raw).expect_err("skewed pin must reject");
        assert!(err.contains("lock-step"), "got: {}", err);
        assert!(err.contains("ported-at"), "got: {}", err);
    }

    #[test]
    fn upstream_pin_rejects_missing_required_field() {
        let raw = r#"
            [bindings.foo]
            crate = "perry-ext-foo"
            lib = "perry_ext_foo"

            [bindings.foo.upstream]
            version = "1.2.3"
            ported-at = "1.2.3"
            date = "2026-07-29"
        "#;
        let err = parse_well_known_toml(raw).expect_err("missing sha256 must reject");
        assert!(err.contains("sha256"), "got: {}", err);
    }

    /// Every shipped binding must carry an upstream pin — the port map
    /// is TOTAL, so a new binding can't land silently unpinned. (Same
    /// rule as `every_entry_references_a_workspace_crate` above.)
    #[test]
    fn every_entry_carries_an_upstream_pin() {
        let unpinned: Vec<&str> = iter_well_known()
            .filter(|b| b.upstream.is_none())
            // Exempt: Node builtins (upstream is Node core, not npm), aliases
            // (share the aliased binding's pin), and perry-owned packages.
            .filter(|b| !b.node_builtin && b.alias_of.is_none())
            .filter(|b| !b.package.starts_with("@perryts/") && !b.package.starts_with("perry/"))
            .map(|b| b.package.as_str())
            .collect();
        assert!(
            unpinned.is_empty(),
            "bindings without an [bindings.<name>.upstream] pin — provision one with \
             `node scripts/binding_pins.mjs --set <name>`:\n  {}",
            unpinned.join("\n  ")
        );
    }

    /// Every `alias-of` must point at a binding that actually exists —
    /// a dangling alias would resolve to a real crate (via its own
    /// crate/lib fields) but claim provenance from a phantom parent.
    #[test]
    fn alias_of_targets_exist() {
        for b in iter_well_known() {
            if let Some(target) = &b.alias_of {
                assert!(
                    lookup_well_known(target).is_some(),
                    "binding `{}` is alias-of `{}`, which is not a known binding",
                    b.package,
                    target
                );
            }
        }
    }
}
