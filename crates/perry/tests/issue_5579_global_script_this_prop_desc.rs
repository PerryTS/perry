//! Regression (#5579): in *global-script* mode the module top-level `this`
//! must be `globalThis`, so the standard Test262 `verifyProperty` shape over
//! global builtins (`verifyProperty(this, "decodeURI", ...)` /
//! `verifyPrimordialCallableProperty(this, "parseFloat", ...)`) resolves.
//!
//! Root cause: #4868 lowered module top-level `this` to a CJS `module.exports`
//! stand-in (a fresh `{}`) to match the Test262 Node oracle, which back then
//! ran each assembled case as a CommonJS module (`this === module.exports`).
//! #5346/#5511 switched that oracle to a conforming *global script*
//! (`vm.runInThisContext`, where `this === globalThis`). Perry kept emitting the
//! `{}` stand-in, so `this["parseFloat"]` was `undefined` and
//! `Object.getOwnPropertyDescriptor(this, "decodeURI")` was `undefined` —
//! 78 `built-ins/*/prop-desc.js` (and friends) regressed.
//!
//! Fix: `PERRY_GLOBAL_SCRIPT_THIS=1` opts a compile into global-script
//! semantics — top-level `this` lowers to `globalThis`. It is opt-in: the
//! default stays the CJS `{}` stand-in so a standalone build keeps matching
//! `node --experimental-strip-types <file>` (which runs the file as a CJS
//! module, where top-level `this` is NOT `globalThis`). The Test262 harness
//! sets the flag so Perry matches the script-mode Node oracle.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `src` and run it. `global_script` toggles `PERRY_GLOBAL_SCRIPT_THIS`.
fn compile_and_run(src: &str, global_script: bool) -> (bool, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write entry");
    let output = dir.path().join("main_bin");

    let mut compile = Command::new(perry_bin());
    compile
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        // Keep codegen deterministic and skip the whole-program optimize step
        // (mirrors the Test262 harness compile env).
        .env("PERRY_NO_AUTO_OPTIMIZE", "1");
    if global_script {
        compile.env("PERRY_GLOBAL_SCRIPT_THIS", "1");
    } else {
        // Be explicit so an ambient value in the test env can't leak in.
        compile.env_remove("PERRY_GLOBAL_SCRIPT_THIS");
    }
    let compiled = compile.output().expect("run perry compile");
    assert!(
        compiled.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

/// The actual shape `propertyHelper.js` checks for a primordial global builtin:
/// an own, writable, non-enumerable, configurable function property of `this`.
const PROBE: &str = r#"
const hasOwn = Object.prototype.hasOwnProperty;
console.log("this===globalThis:", (this as any) === globalThis);
console.log("typeof.this.parseFloat:", typeof (this as any)["parseFloat"]);
console.log("own.decodeURI:", hasOwn.call(this, "decodeURI"));
const d = Object.getOwnPropertyDescriptor(this as any, "decodeURI");
console.log("desc.decodeURI:", d
  ? `${d.writable}/${d.enumerable}/${d.configurable}/${typeof d.value}`
  : "undefined");
console.log("DONE");
"#;

#[test]
fn global_script_mode_top_level_this_is_global_this() {
    let (ok, out) = compile_and_run(PROBE, /* global_script */ true);
    assert!(ok, "compiled binary did not exit cleanly\n{out}");
    assert!(
        out.contains("this===globalThis: true"),
        "global-script top-level `this` must be `globalThis`\n{out}"
    );
    assert!(
        out.contains("typeof.this.parseFloat: function"),
        "`this.parseFloat` must read back as a function (verifyPrimordialCallableProperty)\n{out}"
    );
    assert!(
        out.contains("own.decodeURI: true"),
        "`decodeURI` must be an own property of `this` (verifyProperty)\n{out}"
    );
    assert!(
        out.contains("desc.decodeURI: true/false/true/function"),
        "global builtin descriptor must be {{writable, !enumerable, configurable, function}}\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}

#[test]
fn default_mode_top_level_this_stays_cjs_exports() {
    // Without the flag, top-level `this` must remain the CJS `module.exports`
    // stand-in (`{}`) so standalone builds keep matching the default
    // `node --experimental-strip-types` (CommonJS) parity oracle.
    let (ok, out) = compile_and_run(PROBE, /* global_script */ false);
    assert!(ok, "compiled binary did not exit cleanly\n{out}");
    assert!(
        out.contains("this===globalThis: false"),
        "default top-level `this` must NOT be `globalThis`\n{out}"
    );
    assert!(
        out.contains("typeof.this.parseFloat: undefined"),
        "default top-level `this` is a fresh object: `this.parseFloat` is undefined\n{out}"
    );
    assert!(
        out.contains("own.decodeURI: false"),
        "default top-level `this` owns no global builtins\n{out}"
    );
    assert!(
        out.contains("desc.decodeURI: undefined"),
        "default top-level `this` has no `decodeURI` descriptor\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}

#[test]
fn global_script_mode_direct_eval_this_matches_this() {
    // A conforming host evaluates `eval("this")` in the caller's `this`, so at
    // global-script top level `eval("this") === this === globalThis`. (test262
    // language/eval-code/direct/this-value-global.)
    let src = r#"
console.log("eval.this===globalThis:", (eval("this") as any) === globalThis);
console.log("eval.this===this:", (eval("this") as any) === (this as any));
console.log("DONE");
"#;
    let (ok, out) = compile_and_run(src, /* global_script */ true);
    assert!(ok, "compiled binary did not exit cleanly\n{out}");
    assert!(
        out.contains("eval.this===globalThis: true"),
        "direct `eval(\"this\")` must fold to `globalThis` in global-script mode\n{out}"
    );
    assert!(
        out.contains("eval.this===this: true"),
        "`eval(\"this\") === this` must hold in global-script mode\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}
