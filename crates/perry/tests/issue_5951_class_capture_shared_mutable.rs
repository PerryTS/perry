//! #5951 — class field-init/method closures that capture an outer local.
//!
//! Perry lifts a class out of its declaring function and threads captured
//! outer locals to the class through the VALUE-based `__perry_cap_*` snapshot
//! machinery (see `crates/perry-hir/src/lower_decl/class_captures.rs`). That is
//! correct for IMMUTABLE / read-only captures — the four `*_value_capture_*`
//! tests below lock those in and MUST stay green.
//!
//! It was WRONG when the captured local is a SHARED MUTABLE cell — mutated by a
//! field-init closure and/or by the declaring function after capture. The
//! snapshot gave the closure its own copy, split from the declaring function's
//! binding, so the two never observed each other's writes. The `shared_mutable_*`
//! tests below lock in the fix: `shared_mutable_capture.rs` desugars such a
//! capture to a one-element array box, which the machinery already captures by
//! POINTER — so the declaring function, every instance, and the closures share
//! one `[0]` cell, matching a normal (non-class-lifted) closure.

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
#[test]
fn shared_mutable_interleaved() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=0;class T{inc=()=>{c++;return c;};}const t=new T();console.log(t.inc(),c,t.inc(),c);}f();",
    );
    assert_eq!(out, "1 1 2 2\n");
}

/// Sibling instances share the SAME captured cell (it is the declaring
/// function's binding, captured by reference). node: `1 2 2`.
#[test]
fn shared_mutable_across_instances() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=0;class T{b=()=>{c+=1;return c;};}const t1=new T();const t2=new T();console.log(t1.b(),t2.b(),c);}f();",
    );
    assert_eq!(out, "1 2 2\n");
}

/// A non-numeric (string) shared capture: the array box must be retyped so the
/// string-typed capture holder does not mangle the array handle.
#[test]
fn shared_mutable_string_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let s=\"x\";class T{app=()=>{s+=\"y\";return s;};}const t=new T();t.app();console.log(s,t.app());}f();",
    );
    assert_eq!(out, "xy xyy\n");
}

/// A plain method (not a field-init arrow) mutating the shared capture.
#[test]
fn shared_mutable_method_writes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        "function f(){let c=0;class T{inc(){c+=1;return c;}}const t=new T();t.inc();console.log(c,t.inc());}f();",
    );
    assert_eq!(out, "1 2\n");
}
