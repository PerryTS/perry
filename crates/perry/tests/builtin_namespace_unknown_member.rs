//! Regression: an unknown property on a builtin global namespace object
//! (`Reflect.enumerate`, `Math.bogus`, `JSON.bogus`, …) must read as
//! `undefined`, not the number `0`.
//!
//! The codegen value-read fall-through for a `GlobalGet` receiver
//! (`property_get.rs`) previously lowered any unrecognized member to `0.0`, so
//! `typeof Math.bogus === "number"` and `Reflect.enumerate === undefined` was
//! `false` — breaking feature-detection. test262:
//! `built-ins/Reflect/enumerate/undefined.js`. Issue #5347.
//!
//! Uses a `.js` fixture (not `.ts`) so a bare unknown-member access like
//! `Math.bogus` is valid source — it exercises the statically-typed
//! builtin-namespace member path that the bug lived in (an `as any` cast would
//! bypass it through dynamic property lookup).

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Write `entry` into `dir`, compile it with `--no-cache`
/// `PERRY_NO_AUTO_OPTIMIZE=1` (links the prebuilt runtime archive), run it, and
/// return stdout. Asserts compile + run succeed. Mirrors the helper in
/// `functional_batch2_regressions.rs`.
fn compile_and_run_js(dir: &Path, entry: &str, source: &str) -> String {
    let entry_path = dir.join(entry);
    std::fs::write(&entry_path, source).expect("write fixture");
    let output = dir.join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry_path)
        .arg("--no-cache")
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn unknown_builtin_namespace_member_is_undefined_not_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_js(
        dir.path(),
        "main.js",
        r#"
// Unknown members on builtin namespace globals → undefined, not 0.
console.log("math-bogus-undef:", Math.bogus === undefined);
console.log("math-bogus-typeof:", typeof Math.bogus);
console.log("math-bogus-not-zero:", Math.bogus !== 0);
console.log("json-bogus-undef:", JSON.bogus === undefined);
console.log("object-bogus-undef:", Object.bogus === undefined);
console.log("number-bogus-undef:", Number.bogus === undefined);
// The motivating test262 case: Reflect.enumerate was removed from the spec.
console.log("reflect-enumerate-undef:", Reflect.enumerate === undefined);
console.log("reflect-no-own-enumerate:", Reflect.hasOwnProperty("enumerate"));
// Known members must still resolve (no regression on special-cased reads).
console.log("math-pi:", Math.PI > 3.14159 && Math.PI < 3.1416);
console.log("reflect-get:", typeof Reflect.get);
console.log("json-stringify:", typeof JSON.stringify);
console.log("promise-resolve:", typeof Promise.resolve);
console.log("number-max:", Number.MAX_SAFE_INTEGER === 9007199254740991);
"#,
    );
    let expected = "math-bogus-undef: true\n\
        math-bogus-typeof: undefined\n\
        math-bogus-not-zero: true\n\
        json-bogus-undef: true\n\
        object-bogus-undef: true\n\
        number-bogus-undef: true\n\
        reflect-enumerate-undef: true\n\
        reflect-no-own-enumerate: false\n\
        math-pi: true\n\
        reflect-get: function\n\
        json-stringify: function\n\
        promise-resolve: function\n\
        number-max: true\n";
    assert_eq!(stdout, expected);
}
