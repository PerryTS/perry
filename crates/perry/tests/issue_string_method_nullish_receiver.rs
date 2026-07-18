//! Inline-lowered `String.prototype` methods on a nullish receiver coerced the
//! receiver to `"undefined"` / `"null"` instead of throwing the member-access
//! `TypeError`.
//!
//! `codegen`'s `lower_string_method` optimistically routes an any-typed
//! receiver (`(x: any).charAt(i)` / `.codePointAt(i)` / `.split(s)` / …) through
//! the inline string helpers, applying `ToString(this)` for a non-string value.
//! But `ToString` maps `undefined`→`"undefined"` and `null`→`"null"`, so
//! `undefined.codePointAt(0)` returned `117` (`"undefined".codePointAt(0)`) and
//! `undefined.toUpperCase()` returned `"UNDEFINED"` — where V8 throws
//! `Cannot read properties of undefined (reading 'codePointAt')`, because
//! `x.codePointAt` reads the method off `x` FIRST (ECMA-262 §13.3, evaluated
//! before the call). The general property-get path (used for e.g. `slice`,
//! `indexOf`) already threw correctly; the inline char-access/case/split path
//! skipped the `RequireObjectCoercible` guard.
//!
//! Fix: the coercion branch calls `js_string_coerce_method_this`, which does
//! `RequireObjectCoercible(this)` (throwing the V8 member-access message with
//! the static method name) before `ToString`. A statically string-typed
//! receiver still skips the guard (fast path).

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
        "compiled binary exited non-zero: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).trim().to_string()
}

#[test]
fn string_method_on_nullish_receiver_throws_member_access_type_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
// `any`-typed receivers read out of a data structure, mirroring the real
// failure shape (`cell.value.codePointAt(0)` where `cell.value` is undefined).
const cells: { value: any }[] = [{ value: undefined }, { value: null }, { value: "abc" }];

function attempt(label: string, fn: () => any): string {
  try {
    const r = fn();
    return label + "=NOTHROW:" + String(r);
  } catch (e: any) {
    return label + "=" + e.message;
  }
}

const lines = [
  attempt("u.codePointAt", () => cells[0].value.codePointAt(0)),
  attempt("u.charCodeAt", () => cells[0].value.charCodeAt(0)),
  attempt("u.charAt", () => cells[0].value.charAt(0)),
  attempt("u.toUpperCase", () => cells[0].value.toUpperCase()),
  attempt("u.split", () => cells[0].value.split("")),
  attempt("n.codePointAt", () => cells[1].value.codePointAt(0)),
  attempt("n.charAt", () => cells[1].value.charAt(0)),
  // A real string receiver still works (no over-eager guard).
  attempt("ok.charAt", () => cells[2].value.charAt(1)),
  attempt("ok.codePointAt", () => cells[2].value.codePointAt(0)),
];

process.stdout.write(lines.join("\n") + "\n");
"#,
    );
    let expected = [
        "u.codePointAt=Cannot read properties of undefined (reading 'codePointAt')",
        "u.charCodeAt=Cannot read properties of undefined (reading 'charCodeAt')",
        "u.charAt=Cannot read properties of undefined (reading 'charAt')",
        "u.toUpperCase=Cannot read properties of undefined (reading 'toUpperCase')",
        "u.split=Cannot read properties of undefined (reading 'split')",
        "n.codePointAt=Cannot read properties of null (reading 'codePointAt')",
        "n.charAt=Cannot read properties of null (reading 'charAt')",
        "ok.charAt=NOTHROW:b",
        "ok.codePointAt=NOTHROW:97",
    ]
    .join("\n");
    assert_eq!(out, expected, "string method on nullish receiver");
}
