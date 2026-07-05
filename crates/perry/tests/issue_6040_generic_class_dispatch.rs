//! Regression tests for generic-class method dispatch on a MONOMORPHIZED
//! receiver (part of the #5917 `generic_class` triage; the scalar-replacement
//! half of `test_edge_interfaces` is tracked separately as #6040).
//!
//! `class C<T> { get(): T {...} }; new C<string>(...).get()` monomorphizes to
//! `C$str`, but the receiver local kept the un-mangled `Generic { base: "C",
//! type_args: [String] }` type. `receiver_class_name` looked up the base
//! `C` (absent from `ctx.classes` — only `C$str` is registered), returned
//! `None`, and dispatch fell to the number/native fast path
//! (`(string).get is not a function`). It now resolves the specialized
//! `base$mangled` name.

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

#[test]
fn generic_class_string_arg_method_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        r#"
class SimpleContainer<T> {
    private _value: T;
    constructor(initial: T) { this._value = initial; }
    get(): T { return this._value; }
    set(value: T): void { this._value = value; }
}
const c = new SimpleContainer<string>("hello");
console.log(c.get());
c.set("world");
console.log(c.get());
"#,
    );
    assert_eq!(out, "hello\nworld\n");
}

#[test]
fn generic_class_multi_field_numeric_arg_method_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        r#"
class Multi<T> {
    a: T; b: number;
    constructor(x: T) { this.a = x; this.b = 99; }
    getA(): T { return this.a; }
}
const m = new Multi<number>(7);
console.log(m.getA(), m.b);
"#,
    );
    assert_eq!(out, "7 99\n");
}

/// The single-field NUMERIC generic instance is scalar-replaced down to its
/// raw-f64 field, so the instance becomes the number and method dispatch on
/// it throws. Distinct from the dispatch fix above — tracked as #6040.
#[ignore = "#6040: single-field numeric generic instance scalar-replaced to its raw-f64 field"]
#[test]
fn generic_class_single_field_numeric_arg_method_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = run(
        dir.path(),
        r#"
class SimpleContainer<T> {
    private _value: T;
    constructor(initial: T) { this._value = initial; }
    get(): T { return this._value; }
    set(value: T): void { this._value = value; }
}
const c = new SimpleContainer<number>(0);
c.set(42);
console.log(c.get());
"#,
    );
    assert_eq!(out, "42\n");
}
