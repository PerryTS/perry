//! Second slice of the 2026-07-02 audit's write-path hardening (first slice:
//! spread validation + set-path band + Map pun):
//!
//! - `js_object_set_field_by_name_transition_fast` admitted SSO strings /
//!   tag remnants via a `top16 >= 0x7FF8` catch-all and deref'd a bare
//!   GcHeader floor — a 2–5-char SSO payload (2–5.5TB range) passed the
//!   macOS heap floor and deref'd unmapped memory (the write-side #5429
//!   twin). Now POINTER-tag-only + full handle band + try_read_gc_header;
//!   everything else defers to the full dynamic path.
//! - `structuredClone` deref'd any POINTER payload ≥ 0x10000 as GcHeader
//!   bytes after registry probes — a fetch/zlib registry id crashed. Now
//!   full-band gated + validated header probe.
//! - The unhandled-rejection printer deref'd a POINTER-tagged reason with a
//!   bare `>= 0x10000` — a thrown fetch Response id crashed instead of
//!   printing. Now is_plausible_heap_addr-gated.
//!
//! Expected outputs byte-for-byte vs `node --experimental-strip-types`.

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

/// structuredClone still deep-clones plain object graphs (the validated
/// header probe must not reject genuine heap objects).
#[test]
fn structured_clone_deep_clones_objects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const src = { a: { b: 2 }, n: 5 };
const c: any = structuredClone(src);
c.a.b = 3;
console.log("clone:", JSON.stringify(c), "orig:", JSON.stringify(src));
"#,
    );
    assert_eq!(
        stdout,
        "clone: {\"a\":{\"b\":3},\"n\":5} orig: {\"a\":{\"b\":2},\"n\":5}\n"
    );
}

/// A property write on a dynamically-typed SSO string receiver is a silent
/// no-op per JS (non-strict primitive write) — and must not fault in the
/// transition fast path.
#[test]
fn sso_string_receiver_write_is_silent_noop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const v: any = JSON.parse('{"s":"ab"}').s;
v.x = 1;
console.log("sso-write-ok:", v, typeof v.x);
"#,
    );
    assert_eq!(stdout, "sso-write-ok: ab undefined\n");
}
