//! Regression tests for the 2026-07-02 audit's handle-band/type-confusion
//! write-path P0s:
//!
//! - `{...expr}` (js_object_copy_own_fields) deref'd ANY value ≥ 0x10000 as
//!   an ObjectHeader — spreading a Map walked MapHeader bytes as object
//!   fields, spreading a number/SSO string deref'd non-heap bits (Linux
//!   SIGSEGV). Spec: non-objects and keyless exotics contribute nothing.
//! - The dynamic set path's handle guard was `< 0x10000` (one zero short of
//!   the handle band) and a recognized non-object heap type could fall
//!   through to the plain-object write via the object_type pun: a Map with
//!   EXACTLY one entry has MapHeader.size aliasing object_type ==
//!   OBJECT_TYPE_REGULAR, so `m.customProp = 5` corrupted the Map's bytes.
//!
//! The one-entry Map/Set cases assert SURVIVAL (contents intact, exit 0) —
//! node additionally allows expando reads on Maps (`m.custom` → 5), which
//! Perry does not yet support (ExoticKind::Map is a tracked follow-up); the
//! expando read is therefore deliberately not asserted here.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
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
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Spreading a Map/number yields `{}` (not UB); spreading a plain object
/// still copies its fields.
#[test]
fn spread_of_non_objects_is_empty_and_safe() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const m = new Map([["a", 1]]);
console.log("spread-map:", JSON.stringify({ ...m }));
console.log("spread-num:", JSON.stringify({ ...(5 as any) }));
console.log("spread-obj:", JSON.stringify({ ...{ x: 1, y: 2 } }));
"#,
    );
    assert_eq!(
        stdout,
        "spread-map: {}\nspread-num: {}\nspread-obj: {\"x\":1,\"y\":2}\n"
    );
}

/// A dynamic property write on a ONE-entry Map/Set (the object_type-pun
/// shape) must leave the collection intact instead of corrupting its bytes.
#[test]
fn expando_write_on_one_entry_map_does_not_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const one = new Map([["k", "v"]]);
(one as any).custom = 5;
console.log("map-alive:", one.get("k"), one.size);
const s = new Set([1, 2]);
(s as any).tag = "t";
console.log("set-alive:", s.has(2), s.size);
"#,
    );
    assert_eq!(stdout, "map-alive: v 1\nset-alive: true 2\n");
}
