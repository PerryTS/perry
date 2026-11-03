//! A computed read on a native handle (`headers[key]`) passes the handle's raw
//! registry id as the by-name ABI receiver. The by-name ingress must box that
//! word through the unknown-word constructor before any probe, never through
//! the heap-only `JSValue::pointer`: the contract build (the instrumented
//! archives, which `PERRY_CHECK_CONTRACTS=1` links) aborts on an id-band
//! payload there (`[perry-contract] op=JSValue::pointer P=Heap`). The keyed
//! setter behind `handle[key] ??= v` gets the handle's `HANDLE_TAG` word and
//! must not read it as an array header (SIGSEGV in every build).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const KNOBS: &[&str] = &[
    "PERRY_GC_INSTRUMENTS",
    "PERRY_GC_TRACE",
    "PERRY_CHECK_CONTRACTS",
    "PERRY_GC_SCHEDULE_SEED",
    "PERRY_GC_SCHEDULE_RATE",
];

const READ: &str = r#"const headers: any = new Headers();
const key = process.argv[2] || "missing";
console.log(typeof headers[key]);
"#;

const NULLISH_STORE: &str = r#"const headers: any = new Headers();
const key = process.argv[2] || "missing";
headers[key] ??= 3;
console.log(typeof headers[key]);
"#;

fn compile_contract_build(dir: &Path, source: &str) -> PathBuf {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
    cmd.current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for name in KNOBS {
        cmd.env_remove(name);
    }
    cmd.env("PERRY_CHECK_CONTRACTS", "1");
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    output
}

fn run(binary: &Path, key: &str) -> Output {
    let mut cmd = Command::new(binary);
    for name in KNOBS {
        cmd.env_remove(name);
    }
    // Only the instrumented archives accept a schedule seed at run time; the
    // default ones refuse it. Its report proves the contract build ran.
    cmd.env("PERRY_GC_SCHEDULE_SEED", "7");
    cmd.arg(key).output().expect("run compiled binary")
}

fn assert_runs(source: &str, expected: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let binary = compile_contract_build(dir.path(), source);
    // `missing` is an ordinary miss; `length` is a typed-array metadata name,
    // which the by-name ingress probes before the handle dispatch.
    for key in ["missing", "length"] {
        let out = run(&binary, key);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("[perry-contract]"),
            "contract violated for key {key}: {out:?}"
        );
        assert!(out.status.success(), "key {key}: {out:?}");
        assert!(
            stderr.contains("[gc-schedule] done: seed=7"),
            "the instrumented archives must be linked: {out:?}"
        );
        assert_eq!(String::from_utf8_lossy(&out.stdout), expected, "key {key}");
    }
}

#[test]
fn computed_read_on_a_headers_handle_passes_the_heap_payload_contract() {
    assert_runs(READ, "undefined\n");
}

#[test]
fn nullish_keyed_store_on_a_headers_handle_reaches_the_handle_setter() {
    assert_runs(NULLISH_STORE, "number\n");
}
