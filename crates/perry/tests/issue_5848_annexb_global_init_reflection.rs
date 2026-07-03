//! Regression (#5848): Annex B block-nested top-level `function` declarations
//! (`{ function f(){} }`, `if (c) function f(){}`, `switch (x) { case 1:
//! function f(){} }`, directly in sloppy global code) must have an early,
//! non-configurable, `undefined`-valued own property on `globalThis` — before
//! the block/if/switch statement that declares them ever executes.
//!
//! Root cause: GlobalDeclarationInstantiation's `CreateGlobalVarBinding`
//! (B.3.3.2 step 5.b.i) runs for these legacy-hoisted names before any
//! top-level statement executes, seeding `undefined`. Perry already gave the
//! name a local var slot (`annexb_block_fn_var_ids`, #5297) but never
//! reflected it onto the `globalThis` object early, so
//! `Object.getOwnPropertyDescriptor(globalThis, name)` read `undefined`
//! (missing) instead of a real `{value: undefined, writable: true,
//! enumerable: true, configurable: false}` descriptor ahead of the block.
//!
//! Fix: mirrors #5579's `script_global_functions` reflection (entry.rs
//! `emit_script_global_function_decls`) with a sibling pass
//! (`emit_annexb_global_undefined_decls`) driven by a new
//! `HirModule::annexb_global_undefined_names` list, populated only for names
//! not already covered by a same-named bare top-level function declaration
//! (which already reflects its real value).

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
fn block_nested_function_is_predeclared_undefined_on_globalthis() {
    // Mirrors test262 `annexB/language/global-code/block-decl-global-init.js`.
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const hasOwn = Object.prototype.hasOwnProperty;

// Before the block runs, `f` must already be `undefined` (not a
// ReferenceError) AND a real own property of globalThis.
console.log("pre.f:", f);
console.log("pre.own:", hasOwn.call(globalThis, "f"));
const desc = Object.getOwnPropertyDescriptor(globalThis, "f");
console.log("pre.desc:", JSON.stringify(desc));

{
  function f() { return "declaration"; }
}

// After the block runs, the legacy var must observe the real function
// (unaffected by this fix — pre-existing local-slot behavior, #5297).
console.log("post.typeof:", typeof f);
console.log("post.call:", f());
console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("pre.f: undefined"),
        "`f` must read as undefined before the block runs\n{out}"
    );
    assert!(
        out.contains("pre.own: true"),
        "`f` must already be an own property of globalThis before the block runs\n{out}"
    );
    assert!(
        // `JSON.stringify` drops the `value` key since it's `undefined`.
        out.contains(r#"pre.desc: {"writable":true,"enumerable":true,"configurable":false}"#),
        "descriptor must be {{value: undefined, writable: true, enumerable: true, configurable: false}}\n{out}"
    );
    assert!(
        out.contains("post.typeof: function"),
        "`f` must be callable after the block declares it\n{out}"
    );
    assert!(
        out.contains("post.call: declaration"),
        "the block-declared function must be the one actually called\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}

#[test]
fn if_and_switch_nested_function_predeclared_undefined_on_globalthis() {
    // Mirrors `if-decl-else-decl-a-global-init.js` / `switch-case-global-init.js`.
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
const hasOwn = Object.prototype.hasOwnProperty;

console.log("pre.own.g:", hasOwn.call(globalThis, "g"));
console.log("pre.own.h:", hasOwn.call(globalThis, "h"));

if (true) function g() {} else function _g() {}

switch (1) {
  case 1:
    function h() {}
}

console.log("post.typeof.g:", typeof g);
console.log("post.typeof.h:", typeof h);
console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("pre.own.g: true"),
        "`g` (if-decl) must already be an own property of globalThis before it runs\n{out}"
    );
    assert!(
        out.contains("pre.own.h: true"),
        "`h` (switch-case) must already be an own property of globalThis before it runs\n{out}"
    );
    assert!(out.contains("post.typeof.g: function"));
    assert!(out.contains("post.typeof.h: function"));
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}

#[test]
fn same_named_top_level_function_is_not_shadowed_by_undefined_reflection() {
    // A same-named bare top-level function declaration already reflects its
    // real value via `script_global_functions` — the new annexB undefined
    // reflection must skip that name, not clobber it back to `undefined`.
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
function f() { return "top-level"; }

const hasOwn = Object.prototype.hasOwnProperty;
console.log("pre.own:", hasOwn.call(globalThis, "f"));
console.log("pre.value:", (globalThis as any).f());

{
  function f() { return "block"; }
}

console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(ok, "compiled binary did not exit cleanly\nstdout:\n{out}");
    assert!(
        out.contains("pre.own: true"),
        "top-level `f` must be an own globalThis property\n{out}"
    );
    assert!(
        out.contains("pre.value: top-level"),
        "the real top-level function value must NOT be clobbered to undefined\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}
