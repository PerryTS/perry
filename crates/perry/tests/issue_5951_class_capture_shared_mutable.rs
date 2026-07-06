//! #5951 — class field-init/method closures that capture an outer local.
//!
//! Perry lifts a class out of its declaring function and threads captured
//! outer locals to the class through the VALUE-based `__perry_cap_*` snapshot
//! machinery (see `crates/perry-hir/src/lower_decl/class_captures.rs`). That is
//! correct for IMMUTABLE / read-only captures — the four `*_value_capture_*`
//! tests below lock those in and MUST stay green.
//!
//! It is WRONG when the captured local is a SHARED MUTABLE cell — mutated by a
//! field-init closure and/or by the declaring function after capture. The
//! snapshot gives the closure its own copy, split from the declaring function's
//! binding, so the two never observe each other's writes. The three
//! `shared_mutable_*` tests below capture that bug (verified vs
//! `node --experimental-strip-types`) and are `#[ignore]`d pending the
//! boxed-capture-mode fix designed on the issue: the fix must box the local in
//! the declaring function and thread a box POINTER (not a value snapshot)
//! through `__perry_cap_*`, with the closure dereferencing it — bringing this
//! into line with how a normal (non-class-lifted) closure already shares a
//! mutable capture. Un-ignore each as it lands.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run(dir: &std::path::Path, src: &str) -> String {
    let entry = dir.join("main.ts");
    let out = dir.join("main_bin");
    std::fs::write(&entry, src).expect("write");
    let c = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .arg("--no-cache")
        .output()
        .expect("compile");
    assert!(
        c.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&out).current_dir(dir).output().expect("run");
    assert!(
        r.status.success(),
        "run failed\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).into_owned()
}

// ---- Value-based captures that MUST stay correct (the safety boundary) ----

#[test]
fn value_capture_readonly_field_closure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=7;class T{b=()=>c*2;}const t=new T();console.log(t.b(),c);}f();",
    );
    assert_eq!(out, "14 7\n");
}

#[test]
fn value_capture_immutable_const() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){const k=42;class T{b=()=>k;}console.log(new T().b());}f();",
    );
    assert_eq!(out, "42\n");
}

#[test]
fn value_capture_in_constructor() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(x:number){class T{v:number;constructor(){this.v=x*10;}}console.log(new T().v);}f(5);",
    );
    assert_eq!(out, "50\n");
}

#[test]
fn value_capture_in_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=3;class T{get(){return c;}}console.log(new T().get());}f();",
    );
    assert_eq!(out, "3\n");
}

// ---- Shared MUTABLE captures — the #5951 bug (currently wrong) ----

/// Field-init closure mutates the capture; the declaring function reads it.
/// node: `2 3`; perry: `0 3` (closure mutates its own split copy).
#[ignore = "#5951: shared-mutable class capture gets a split cell (needs boxed-capture mode)"]
#[test]
fn shared_mutable_closure_writes_fn_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=0;class T{b=()=>{c+=1;return c;};}const t=new T();t.b();t.b();console.log(c,t.b());}f();",
    );
    assert_eq!(out, "2 3\n");
}

/// Declaring function mutates the capture after construction; the closure
/// (read-only) must observe it. node: `99 99`; perry: `1 99`.
#[ignore = "#5951: shared-mutable class capture gets a split cell (needs boxed-capture mode)"]
#[test]
fn shared_mutable_fn_writes_closure_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=1;class T{b=()=>c;}const t=new T();c=99;console.log(t.b(),c);}f();",
    );
    assert_eq!(out, "99 99\n");
}

/// Interleaved: closure increments, declaring function reads between calls.
/// node: `1 1 2 2`; perry: `1 0 2 0`.
#[ignore = "#5951: shared-mutable class capture gets a split cell (needs boxed-capture mode)"]
#[test]
fn shared_mutable_interleaved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=0;class T{inc=()=>{c++;return c;};}const t=new T();console.log(t.inc(),c,t.inc(),c);}f();",
    );
    assert_eq!(out, "1 1 2 2\n");
}
