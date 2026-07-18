//! #6303 — every `perry-ext-*` crate that depends on `perry-runtime` must build
//! it with perry-runtime's `default` feature set.
//!
//! Why this is a *correctness* invariant and not a style rule:
//!
//! * The `perry-ext-*` crates are `crate-type = ["staticlib", "rlib"]`. A Rust
//!   staticlib **bundles its upstream rlib objects**, so `libperry_ext_http.a`
//!   physically contains a copy of perry-runtime's codegen units.
//! * `perry compile` links the ext archives *before* stdlib/runtime (see
//!   `optimized_libs::no_auto::resolve_no_auto_optimized_libs` →
//!   `prefer_well_known_before_stdlib`). The linker resolves each undefined
//!   symbol from the first archive that defines it, so the copy bundled inside
//!   `libperry_ext_*.a` **wins** for every symbol it exports.
//! * The workspace dependency is declared `default-features = false`
//!   (root `Cargo.toml`). So a *per-crate* `cargo build -p perry-ext-http` —
//!   which is exactly what `.github/workflows/release-packages.yml` does in its
//!   per-crate ext loop — resolves perry-runtime with `regex-engine`,
//!   `temporal`, `url-engine`, … switched OFF.
//! * Crucially, the dispatchers those features gate are exported
//!   **unconditionally** (`js_string_replace_search_dyn`,
//!   `js_native_call_method`, …): the feature gate sits *inside the function
//!   body*. A feature-stripped copy therefore still defines the symbol, but with
//!   the RegExp/Temporal detection `#[cfg]`-ed out — it silently ToString-coerces
//!   a RegExp argument and searches for `"/re/g"` **literally**.
//!
//! Net effect of a violation: `str.replace(re, fn)` where codegen cannot
//! statically prove `re` is a RegExp (a module-level `var`, an object property,
//! a function parameter) matches nothing and **never invokes the callback** — a
//! silent wrong answer, not a crash. That is what stopped Express from booting
//! (`get-intrinsic`'s `stringToPath` returned `[]` → `%%` → SyntaxError).
//!
//! Keeping the bundled copy feature-identical to the shipped one makes the
//! duplicate harmless whichever archive the linker happens to pick.
//!
//! #6314 — the same crates must ALSO build perry-runtime with the `stdlib`
//! feature. #6303 only made the bundled copy *semantically* identical; a
//! feature-identical copy still **defines** perry-runtime's no-op `stdlib_stubs`
//! (`js_stdlib_init_dispatch`, `js_stdlib_process_pending`, the fetch/ws/readline
//! no-ops), which are gated `#[cfg(not(feature = "stdlib"))]`. Bundled and linked
//! before the real perry-stdlib, the no-op `js_stdlib_init_dispatch` wins
//! first-definition, so perry-stdlib's real dispatch never runs and a node:http
//! server's tokio reactor is never registered (it dies on its first accept). The
//! link-time strip that is *supposed* to drop those bundled members silently
//! no-ops when perry can't find LLVM `nm`/`objcopy` (e.g. a stock macOS host), so
//! the fix is to not emit the stubs into these copies at all. The `stdlib`
//! feature only gates out that stubs module (perry-stdlib itself uses it for the
//! identical reason); the standalone `libperry_runtime.a` keeps the stubs so
//! runtime-only builds still link.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <root>/crates/perry
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// The `perry-runtime = ...` dependency line of a manifest, if there is one.
/// Only the `[dependencies]`-style `perry-runtime = { ... }` / `perry-runtime.workspace`
/// forms appear in these crates, so a line-prefix match is sufficient and keeps
/// this test dependency-free (no toml parser needed).
fn perry_runtime_dep_line(manifest: &str) -> Option<String> {
    manifest
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("perry-runtime.workspace") || l.starts_with("perry-runtime ="))
        .map(str::to_string)
}

#[test]
fn ext_crates_bundle_a_full_featured_perry_runtime() {
    let root = workspace_root();
    let crates_dir = root.join("crates");
    let mut checked = 0usize;
    let mut violations: Vec<String> = Vec::new();
    let mut stdlib_violations: Vec<String> = Vec::new();

    let entries = std::fs::read_dir(&crates_dir).expect("read crates/");
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("perry-ext-") {
            continue;
        }
        let manifest_path = entry.path().join("Cargo.toml");
        let Ok(manifest) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Some(dep_line) = perry_runtime_dep_line(&manifest) else {
            // Crate doesn't link perry-runtime at all (perry-ffi only) — it
            // cannot bundle a divergent copy, so it is not in scope.
            continue;
        };
        checked += 1;

        // `perry-runtime.workspace = true` inherits `default-features = false`
        // from the workspace dep and adds nothing back — always a violation.
        // Otherwise the explicit `features = [...]` list must contain "default".
        let has_features = dep_line.contains("features");
        if !(has_features && dep_line.contains("\"default\"")) {
            violations.push(format!("  {name}: {dep_line}"));
        }
        // #6314: must also carry "stdlib" so the bundled copy does not export
        // perry-runtime's no-op stdlib_stubs and shadow perry-stdlib's real ones.
        if !(has_features && dep_line.contains("\"stdlib\"")) {
            stdlib_violations.push(format!("  {name}: {dep_line}"));
        }
    }

    assert!(
        checked > 0,
        "found no perry-ext-* crate depending on perry-runtime — did the crate \
         layout change? This guard would silently pass forever."
    );

    assert!(
        violations.is_empty(),
        "#6303: these perry-ext-* crates bundle a feature-stripped perry-runtime \
         into their staticlib.\n\
         They are linked BEFORE stdlib/runtime, so their copy wins the link and \
         silently degrades every\n\
         unconditionally-exported dispatcher whose body is feature-gated \
         (js_string_replace_search_dyn,\n\
         js_native_call_method, ...) — e.g. `str.replace(re, fn)` stops invoking \
         its callback.\n\
         Add \"default\" to the perry-runtime `features` list:\n{}",
        violations.join("\n")
    );

    assert!(
        stdlib_violations.is_empty(),
        "#6314: these perry-ext-* crates bundle a perry-runtime that still exports \
         the no-op `stdlib_stubs`.\n\
         They are linked BEFORE stdlib, so the bundled no-op \
         `js_stdlib_init_dispatch` wins the link and\n\
         perry-stdlib's real dispatch never runs — a node:http server never \
         registers its tokio reactor and\n\
         dies on the first accept. The link-time strip that should drop those \
         members silently no-ops when\n\
         perry can't find LLVM nm/objcopy (e.g. a stock macOS host), so the copy \
         must not emit the stubs.\n\
         Add \"stdlib\" to the perry-runtime `features` list:\n{}",
        stdlib_violations.join("\n")
    );
}
