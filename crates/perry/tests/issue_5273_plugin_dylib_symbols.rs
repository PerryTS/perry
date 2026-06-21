//! Regression test for #5273: a plugin compiled with `--output-type dylib`
//! must export the plugin ABI symbols the runtime resolves by name —
//! `perry_plugin_abi_version`, `plugin_activate`, and (when the user exports
//! `deactivate`) `plugin_deactivate`.
//!
//! The runtime loader (`crates/perry-runtime/src/plugin.rs`) `dlsym`'s exactly
//! these names; without them `perry_plugin_load` hits the
//! `[plugin] No plugin_activate symbol` path and returns 0, so the documented
//! plugin system could not load a plugin on any platform.
//!
//! The codegen for these symbols existed but lived in the *non-entry-module*
//! branch of `compile_module_entry` and produced malformed IR (an unnamed
//! parameter), so it never actually ran: a single-file plugin IS the entry
//! module, so it received none of the three symbols. The fix emits the shim
//! once, from the entry module — see `crates/perry-codegen/src/codegen/entry.rs`
//! (`emit_plugin_abi_shim`).
//!
//! Unix-only: the assertions read the emitted shared library's exported symbol
//! table with `nm`. A raw byte-scan is not reliable here because macOS stores
//! exported names in the Mach-O export trie (not contiguous ASCII). The
//! Windows export path (`.def` generation in
//! `crates/perry/src/commands/compile.rs`) is gated by the same codegen and
//! covered by `tests/test_plugin_lifecycle.sh`.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn dylib_ext() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// Compile `source` as a dylib in `root` and return the output path.
fn compile_plugin_dylib(root: &Path, source: &str) -> PathBuf {
    let entry = root.join("plugin.ts");
    std::fs::write(&entry, source).expect("write plugin source");
    let output = root.join(format!("plugin.{}", dylib_ext()));
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("--output-type")
        .arg("dylib")
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile --output-type dylib failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(output.exists(), "dylib was not produced at {:?}", output);
    output
}

/// Return the set of externally-defined symbol names exported by `lib`, with
/// any platform leading underscore (Mach-O) stripped. Skips the test with a
/// panic-free `return None` if `nm` is unavailable.
fn exported_symbols(lib: &Path) -> Option<Vec<String>> {
    // BSD nm (macOS) and GNU nm (Linux) both accept `-g` (external only).
    // `-U` means "defined only" on both. On ELF the dynamic table is what the
    // loader sees, so also pass `-D` on Linux; macOS BSD nm rejects `-D`, so
    // only add it off-macOS.
    let mut cmd = Command::new("nm");
    cmd.arg("-gU");
    if !cfg!(target_os = "macos") {
        cmd.arg("-D");
    }
    let out = cmd.arg(lib).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let syms = text
        .lines()
        .filter_map(|line| line.split_whitespace().last())
        .map(|name| name.strip_prefix('_').unwrap_or(name).to_string())
        .collect();
    Some(syms)
}

const PLUGIN_WITH_DEACTIVATE: &str = r#"
import type { PluginApi } from "perry/plugin"

export function activate(api: PluginApi) {
    api.setMetadata("counter", "1.0.0", "Counts hook invocations")
}

export function deactivate() {
    // teardown
}
"#;

const PLUGIN_WITHOUT_DEACTIVATE: &str = r#"
import type { PluginApi } from "perry/plugin"

export function activate(api: PluginApi) {
    api.setMetadata("counter", "1.0.0", "no deactivate export")
}
"#;

#[test]
fn plugin_dylib_exports_abi_activate_and_deactivate() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lib = compile_plugin_dylib(dir.path(), PLUGIN_WITH_DEACTIVATE);

    let Some(syms) = exported_symbols(&lib) else {
        eprintln!("SKIP: `nm` unavailable; cannot inspect exported symbols");
        return;
    };
    let has = |s: &str| syms.iter().any(|sym| sym == s);

    assert!(
        has("perry_plugin_abi_version"),
        "plugin dylib must export perry_plugin_abi_version (#5273); exports: {syms:?}"
    );
    assert!(
        has("plugin_activate"),
        "plugin dylib must export plugin_activate (#5273); exports: {syms:?}"
    );
    assert!(
        has("plugin_deactivate"),
        "plugin dylib that exports `deactivate` must export plugin_deactivate (#5273); exports: {syms:?}"
    );
}

#[test]
fn plugin_without_deactivate_omits_plugin_deactivate_symbol() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lib = compile_plugin_dylib(dir.path(), PLUGIN_WITHOUT_DEACTIVATE);

    let Some(syms) = exported_symbols(&lib) else {
        eprintln!("SKIP: `nm` unavailable; cannot inspect exported symbols");
        return;
    };
    let has = |s: &str| syms.iter().any(|sym| sym == s);

    // The two mandatory symbols are always present for a plugin entry module.
    assert!(
        has("perry_plugin_abi_version"),
        "plugin dylib must export perry_plugin_abi_version (#5273); exports: {syms:?}"
    );
    assert!(
        has("plugin_activate"),
        "plugin dylib must export plugin_activate (#5273); exports: {syms:?}"
    );
    // `plugin_deactivate` is only emitted when the user exports `deactivate`.
    assert!(
        !has("plugin_deactivate"),
        "plugin_deactivate must not be exported without a user `deactivate`; exports: {syms:?}"
    );
}
