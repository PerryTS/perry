//! Regression test for #5972: `getDeviceModel()` returned `NaN` instead of a
//! JS string, and using that value as an object key segfaulted.
//!
//! Two stacked bugs, both fixed:
//!  1. `perry/system` `getDeviceModel` / `getOSVersion` were declared
//!     `ReturnKind::F64` in the dispatch table, but their runtime fns return a
//!     raw `*mut StringHeader` (i64) via `js_string_from_bytes`. `F64` passed
//!     the pointer bits straight through as a double → `NaN`. Fixed to
//!     `ReturnKind::Str` (NaN-box with STRING_TAG), matching `getLocale`.
//!  2. A property-key expression that yields no usable string handle (e.g.
//!     `js_get_string_pointer_unified` returning 0 for a NaN key) reached
//!     `js_object_get_field_by_name` as a null key, which several arms
//!     dereferenced without a null check → SIGSEGV at offset 4. Now guarded:
//!     a null key misses → `undefined`, per JS semantics.
//!
//! The program mirrors the dbmeter calibration-table shape that first hit this:
//! look up a `Record<string, number>` keyed by `getDeviceModel()` at module
//! top level. On any host it must run without crashing and take the
//! unknown-device default (the CI host's model isn't in the table).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn host_ui_archive_available() -> bool {
    let archive_name = if cfg!(target_os = "linux") {
        "libperry_ui_gtk4.a"
    } else if cfg!(target_os = "macos") {
        "libperry_ui_macos.a"
    } else if cfg!(target_os = "windows") {
        "perry_ui_windows.lib"
    } else {
        return false;
    };

    let mut search_dirs = Vec::new();
    for env_var in ["PERRY_RUNTIME_DIR", "PERRY_LIB_DIR"] {
        if let Some(dir) = std::env::var_os(env_var) {
            search_dirs.push(PathBuf::from(dir));
        }
    }

    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    search_dirs.push(workspace_root.join("target/release"));
    search_dirs.push(workspace_root.join("target/debug"));

    if let Some(bin_dir) = perry_bin().parent() {
        search_dirs.push(bin_dir.to_path_buf());
        if let Some(target_dir) = bin_dir.parent() {
            search_dirs.push(target_dir.join("release"));
            search_dirs.push(target_dir.join("debug"));
        }
    }

    search_dirs
        .into_iter()
        .any(|dir| dir.join(archive_name).is_file())
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary crashed (status {:?}) — #5972 regression\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Part 1: `getDeviceModel()` is a real string, and indexing a
/// `Record<string, number>` with it works (known key hits, unknown misses).
#[test]
fn get_device_model_is_a_string_usable_as_object_key() {
    // The platform implementation lives in libperry_ui_*.a. Generic Perry
    // integration shards intentionally build only runtime + stdlib, so keep
    // the platform end-to-end coverage when that archive is provisioned and
    // rely on perry-dispatch's unit test for the ABI shape everywhere else.
    if !host_ui_archive_available() {
        eprintln!("skipping platform-backed #5972 test: host UI archive is unavailable");
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { getDeviceModel } from "perry/system";

const offsets: Record<string, number> = {
  "iPhone15,2": 1.0,
  "Watch7,5": 1.3,
};

function offsetFor(model: string): number {
  const o = offsets[model];
  return o !== undefined ? o : 0.0;
}

const model = getDeviceModel();
// typeof must be "string", never "number" (was NaN → "number").
console.log("TYPE", typeof model);
// A known key hits; an unknown key misses → default. The host's real
// model is (almost certainly) not in the table, so it takes the default
// WITHOUT crashing — that is the #5972 repro.
console.log("KNOWN", offsetFor("iPhone15,2"));
console.log("HOST", offsetFor(model));
console.log("DONE");
"#,
    );
    assert!(
        stdout.contains("TYPE string"),
        "getDeviceModel must be a string, not NaN: {stdout}"
    );
    assert!(stdout.contains("KNOWN 1"), "known-key lookup: {stdout}");
    assert!(
        stdout.contains("HOST 0"),
        "unknown host model → default 0 without crash: {stdout}"
    );
    assert!(
        stdout.contains("DONE"),
        "reached end without crash: {stdout}"
    );
}

/// Part 2 (defense-in-depth): indexing an object with a numeric / NaN key
/// through a type-erased receiver must never segfault. Per JS the key coerces
/// to a string and the lookup misses → `undefined`.
#[test]
fn numeric_and_nan_object_keys_do_not_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const obj: any = { "1": "one", "NaN": "not-a-number" };

const numKey: any = 1;
const nanKey: any = NaN;
const floatKey: any = 2.5;

// obj[1] → obj["1"]; obj[NaN] → obj["NaN"]; obj[2.5] → obj["2.5"] (miss).
console.log("NUM", obj[numKey]);
console.log("NAN", obj[nanKey]);
console.log("FLOAT", String(obj[floatKey]));
console.log("DONE");
"#,
    );
    assert!(stdout.contains("NUM one"), "numeric key coercion: {stdout}");
    assert!(
        stdout.contains("NAN not-a-number"),
        "NaN key coerces to \"NaN\": {stdout}"
    );
    assert!(
        stdout.contains("FLOAT undefined"),
        "unknown float key misses → undefined: {stdout}"
    );
    assert!(stdout.contains("DONE"), "no crash: {stdout}");
}
