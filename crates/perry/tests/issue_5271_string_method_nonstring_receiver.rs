//! Regression tests for #5271 — a method whose name collides with a
//! `String.prototype` builtin (`trim`/`split`/`slice`/…) on a NON-string
//! receiver was eagerly lowered to the static `String.prototype.<m>` fast
//! path in `lower_string_method`.
//!
//! Root cause: the "string-only method on an `any`-typed receiver" fallback
//! in `lower_call/property_get.rs` forced the String path for collision-prone
//! method names regardless of the receiver's real shape. joi's
//! `validator.js:359` calls its OWN `internals.trim(value, schema)` — 2 args
//! to a 0-arg String builtin — which aborted codegen with
//! `perry-codegen: String.trim takes no args, got 2`. Same-arity collisions
//! (`{ trim() {…} }.trim()`, `{ split(a,b){…} }.split(1,2)`) instead bit-cast
//! the object pointer as a string and returned "[object Object]".
//!
//! The fix gates the static String fast path on (a) the receiver NOT being a
//! known object-literal local and (b) the arg count being plausible for the
//! String builtin; otherwise it falls through to `js_native_call_method`,
//! which resolves the receiver's own member at runtime while still servicing a
//! genuine (any-typed) string receiver via its `jsval.is_string()` arm.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> String {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// The exact joi shape: a user `trim` taking 2 args. Pre-fix this aborted
/// codegen ("String.trim takes no args, got 2"); it must compile and call the
/// object's own method.
#[test]
fn object_trim_two_args_resolves_to_user_method() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const internals = { trim(value: string, schema: any) { return value + ":" + schema; } };
console.log(internals.trim("a", "b"));
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("a:b"),
        "internals.trim(value, schema) must call the object's own 2-arg method (got: {stdout})"
    );
}

/// Same-arity collisions: a user method whose arg count matches the String
/// builtin must still resolve to the object's own member, not the builtin.
#[test]
fn object_methods_with_matching_arity_resolve_to_user_methods() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const a = { trim() { return "USERTRIM"; } };          // 0-arg, matches String.trim
const b = { split(x: number, y: number) { return x + y; } }; // 2-arg, matches String.split
const c = { slice() { return "USERSLICE"; } };        // 0-arg, matches String.slice
console.log(a.trim());
console.log(b.split(1, 2));
console.log(c.slice());
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("USERTRIM"),
        "a.trim() must call the user method (got: {stdout})"
    );
    assert!(
        stdout.contains("\n3"),
        "b.split(1, 2) must call the user method and return 3 (got: {stdout})"
    );
    assert!(
        stdout.contains("USERSLICE"),
        "c.slice() must call the user method (got: {stdout})"
    );
}

/// The fix must NOT regress genuine string receivers: literals and statically
/// string-typed values keep the fast String path with correct results.
#[test]
fn genuine_string_receivers_keep_string_semantics() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
console.log("  hi  ".trim());
console.log(JSON.stringify("a,b".split(",")));
const s: string = "HeLLo";
console.log(s.toLowerCase());
console.log(JSON.stringify(s.split("L")));
// any-typed value that is really a string still works via runtime dispatch
function id(x: any): any { return x; }
const t: any = id("X,Y,Z");
console.log(JSON.stringify(t.split(",")));
console.log(t.trim());
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert!(
        stdout.contains("hi\n"),
        "string literal trim must work (got: {stdout})"
    );
    assert!(
        stdout.contains(r#"["a","b"]"#),
        "string literal split must work (got: {stdout})"
    );
    assert!(
        stdout.contains("hello\n"),
        "string-typed toLowerCase must work (got: {stdout})"
    );
    assert!(
        stdout.contains(r#"["He","","o"]"#),
        "string-typed split must work (got: {stdout})"
    );
    assert!(
        stdout.contains(r#"["X","Y","Z"]"#),
        "any-typed string split must work (got: {stdout})"
    );
    assert!(
        stdout.contains("X,Y,Z"),
        "any-typed string trim must work (got: {stdout})"
    );
}

/// Scalar replacement of a non-escaping split result must preserve the
/// `ToString(this)` coercion used by the normal String-method lowering. This
/// receiver is deliberately `any`-typed and numeric: treating its boxed number
/// as a StringHeader would otherwise produce `undefined` instead of the first
/// split component.
#[test]
fn scalar_split_on_any_receiver_keeps_string_coercion() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function id(x: any): any { return x; }
const value: any = id(12345);
const first = value.split("2")[0];
console.log(first);
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(dir.path(), &entry);
    assert_eq!(
        stdout.trim(),
        "1",
        "split must coerce an any receiver to string"
    );
}
