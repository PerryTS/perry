//! Regression (#5579): bare top-level `function` declarations in a non-ESM
//! entry program (a *Script*) must become own properties of `globalThis`,
//! per GlobalDeclarationInstantiation.
//!
//! Root cause: Perry compiled top-level function declarations into module-scope
//! bindings but never reflected them onto the `globalThis` singleton. That was
//! invisible until #5511 switched the Test262 Node oracle to run cases as a
//! *Script* (`vm.runInThisContext`) instead of a CommonJS module — a conforming
//! host exposes top-level decls on the global object, so the oracle started
//! (correctly) expecting `Object.prototype.hasOwnProperty.call(globalThis, ...)`
//! to be true. The Test262 async harness (`asyncHelpers.js`) gates on exactly
//! that: `asyncTest` throws `"asyncTest called without async flag"` unless
//! `globalThis` owns `$DONE` (defined by `doneprintHandle.js` as a top-level
//! `function`). 43 `language/.../async-function` (and friends) cases regressed
//! to that error.
//!
//! Fix: in the entry-module codegen branch, for a non-ESM program, reflect each
//! bare top-level function declaration onto `globalThis` before user init runs
//! (hoisting). Nested closures and object-literal methods are NOT reflected.
//!
//! NOTE: this is validated by asserting the compiled program's own stdout, not
//! by diffing against `node` — `node --experimental-strip-types <file>` runs the
//! file as a CJS/ESM *module* (which does NOT reflect), so a parity/gap test
//! would compare against the wrong semantics. The authority for these cases is
//! the Test262 Script oracle, whose expectation this test encodes directly.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
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
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

#[test]
fn top_level_functions_are_global_own_properties() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// `$DONE` is the exact shape the Test262 async harness installs via
// doneprintHandle.js — a bare top-level function declaration.
function $DONE(error: any) {
  return error;
}

// Another top-level function, used as a value through globalThis.
function add(a: number, b: number): number {
  return a + b;
}

const hasOwn = Object.prototype.hasOwnProperty;

// 1) The harness's actual gate: $DONE must be an own property of globalThis.
console.log("own.$DONE:", hasOwn.call(globalThis, "$DONE"));
// 2) Reachable as a value through globalThis, typed as a function...
console.log("typeof.add:", typeof (globalThis as any).add);
// 3) ...and callable through that reflected reference.
console.log("call.add:", (globalThis as any).add(2, 3));

// 4) Nested functions and object-literal methods must NOT leak onto globalThis.
function outer() {
  function inner() {}
  return inner;
}
outer();
const obj = { method() { return 1; } };
obj.method();
console.log("own.inner:", hasOwn.call(globalThis, "inner"));
console.log("own.method:", hasOwn.call(globalThis, "method"));

console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("own.$DONE: true"),
        "top-level `function $DONE` must be an own property of globalThis\n{out}"
    );
    assert!(
        out.contains("typeof.add: function"),
        "globalThis.add must read back as a function\n{out}"
    );
    assert!(
        out.contains("call.add: 5"),
        "the reflected globalThis.add must be callable\n{out}"
    );
    assert!(
        out.contains("own.inner: false"),
        "a nested function declaration must NOT be reflected onto globalThis\n{out}"
    );
    assert!(
        out.contains("own.method: false"),
        "an object-literal method must NOT be reflected onto globalThis\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}

#[test]
fn duplicate_top_level_function_last_declaration_wins() {
    // Mirrors test262 `language/global-code/decl-func-dup.js`: the last
    // declaration of a duplicated top-level function name is the one reflected.
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function dup() { return "first"; }
function dup() { return "second"; }

const hasOwn = Object.prototype.hasOwnProperty;
console.log("own.dup:", hasOwn.call(globalThis, "dup"));
console.log("call.dup:", (globalThis as any).dup());
console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("own.dup: true"),
        "duplicate top-level function must still be a globalThis own property\n{out}"
    );
    assert!(
        out.contains("call.dup: second"),
        "the last duplicate declaration must win on globalThis\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}

#[test]
fn esm_entry_does_not_reflect_top_level_functions() {
    // The opposite branch of the `is_esm_entry` guard: a module with
    // import/export syntax is an ESM Module, not a Script, so its top-level
    // `function` declarations bind in the module record and are NOT own
    // properties of the global object. A *named* export (`export const`) is
    // used as the ESM marker because it populates `Module::exports`, which is
    // the signal the codegen gate reads (an empty `export {}` clause leaves the
    // export list empty and is not currently classified as ESM by Perry).
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
export const marker = 1;
function $DONE(error: any) {
  return error;
}
const hasOwn = Object.prototype.hasOwnProperty;
// The function is still callable as a binding (hoisting is unaffected)...
console.log("typeof.$DONE:", typeof $DONE);
// ...but it must NOT leak onto globalThis for an ESM entry.
console.log("own.$DONE:", hasOwn.call(globalThis, "$DONE"));
console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("typeof.$DONE: function"),
        "the function binding must still exist in module scope\n{out}"
    );
    assert!(
        out.contains("own.$DONE: false"),
        "an ESM entry must NOT reflect top-level functions onto globalThis\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}
