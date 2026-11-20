//! Regression (#13558): module top-level `this` is `undefined`.
//!
//! ECMA-262 §16.2.1.6.4 gives a Module Environment Record's `[[ThisValue]]`
//! the initial value `undefined`, so top-level `this` in an ES module is
//! `undefined`. Perry lowered every top-level `this` to the CommonJS
//! `module.exports` stand-in (`Expr::ModuleTopThis`, #4868), which is right for
//! a CommonJS file — `node --experimental-strip-types` runs those as CommonJS
//! and reports `this === module.exports === {}` — but wrong for a module:
//! `perry compile main.mjs` printed `false` where Node prints `true`
//! (test262 `language/module-code/eval-this.js`).
//!
//! The fix routes the decision through the module goal the compiler already
//! derives for strictness (`module_has_module_goal`): ESM format/package
//! context, or in-source `import`/`export` declarations. Everything else keeps
//! the CommonJS stand-in, and the `PERRY_GLOBAL_SCRIPT_THIS` opt-in still turns
//! a CommonJS file's top-level `this` into `globalThis` for the Test262 host.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Write `files` (relative path → contents) into a fresh temp dir, compile
/// `entry`, run it, and return `(exit_success, stdout, stderr)`.
fn compile_and_run(
    files: &[(&str, &str)],
    entry: &str,
    global_script: bool,
) -> (bool, String, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    for (name, contents) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create fixture dir");
        }
        std::fs::write(&path, contents).expect("write fixture");
    }
    let output_bin = dir.path().join("app_bin");

    let mut compile = Command::new(perry_bin());
    compile
        .current_dir(dir.path())
        .arg("compile")
        .arg(dir.path().join(entry))
        .arg("-o")
        .arg(&output_bin)
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

    let run = Command::new(&output_bin)
        .output()
        .expect("run compiled binary");
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
        String::from_utf8_lossy(&run.stderr).to_string(),
    )
}

#[test]
fn mjs_entry_top_level_this_is_undefined() {
    // The exact shape from the issue: no import/export in the source at all,
    // so only the `.mjs` format makes this a module.
    let (ok, out, err) = compile_and_run(
        &[(
            "main.mjs",
            "console.log(this === undefined, typeof this);\n",
        )],
        "main.mjs",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true undefined");
}

#[test]
fn js_entry_with_export_top_level_this_is_undefined() {
    // A `.js` entry in a directory with no package.json is only a module
    // because of its `export` syntax.
    let (ok, out, err) = compile_and_run(
        &[(
            "main.js",
            "export const marker = 1;\nconsole.log(this === undefined);\n",
        )],
        "main.js",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true");
}

#[test]
fn module_type_package_makes_a_ts_entry_undefined() {
    // Node decides the module system for `.ts` from the nearest package.json
    // `"type"`, exactly like `.js`. The repo's own package.json sets
    // `"type": "module"`, so this is also the gap-suite context.
    let (ok, out, err) = compile_and_run(
        &[
            ("package.json", "{\"type\": \"module\"}\n"),
            ("main.ts", "console.log(this === undefined);\n"),
        ],
        "main.ts",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true");
}

#[test]
fn commonjs_entry_keeps_the_exports_stand_in() {
    // #4868/#5579 preserved: with no package.json a `.ts` entry is CommonJS
    // under Node, where top-level `this` is the `module.exports` stand-in (a
    // plain object), never `globalThis`.
    let (ok, out, err) = compile_and_run(
        &[(
            "main.ts",
            "const t = this as any;\nconsole.log(typeof t, t === undefined, t === globalThis);\n",
        )],
        "main.ts",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "object false false");
}

#[test]
fn global_script_mode_does_not_override_the_module_goal() {
    // The Test262 runner compiles every case with PERRY_GLOBAL_SCRIPT_THIS=1 so
    // a *Script* case sees `this === globalThis`. A module is a module whatever
    // host evaluates it, so `language/module-code/eval-this.js` must still see
    // `undefined`.
    let (ok, out, err) = compile_and_run(
        &[("main.mjs", "console.log(this === undefined);\n")],
        "main.mjs",
        true,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true");

    // And a CommonJS entry under the same flag keeps the script semantics.
    let (ok, out, err) = compile_and_run(
        &[("main.ts", "console.log(this === globalThis);\n")],
        "main.ts",
        true,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true");
}

#[test]
fn imported_module_top_level_this_is_undefined() {
    // The module goal is per module, not per program: a non-entry module's
    // top-level `this` is `undefined` too, and only the entry's own format is
    // irrelevant here (the importer is a `.mjs` as well).
    let (ok, out, err) = compile_and_run(
        &[
            (
                "dep.mjs",
                "console.log(\"dep\", this === undefined);\nexport const value = 1;\n",
            ),
            (
                "main.mjs",
                "import { value } from './dep.mjs';\nconsole.log(\"main\", this === undefined, value);\n",
            ),
        ],
        "main.mjs",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        vec!["dep true", "main true 1"]
    );
}

#[test]
fn lexical_this_and_direct_eval_agree_at_module_top_level() {
    // `this` is lexical for an arrow and inherited by direct eval, so both
    // observe the module's `undefined` rather than a synthesized object.
    let (ok, out, err) = compile_and_run(
        &[(
            "main.mjs",
            "const viaArrow = () => this;\nconst viaEval = eval('this');\nconsole.log(viaArrow() === undefined, viaEval === undefined);\n",
        )],
        "main.mjs",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true true");
}

#[test]
fn writing_a_property_of_module_top_level_this_throws() {
    // Module code is strict (ES2024 §11.2.2), so a property write on the
    // `undefined` `this` is a TypeError, not a silent no-op.
    let (ok, out, err) = compile_and_run(
        &[(
            "main.mjs",
            "try {\n  (this as any).marker = 1;\n  console.log('no throw');\n} catch (e) {\n  console.log(e instanceof TypeError);\n}\n",
        )],
        "main.mjs",
        false,
    );
    assert!(
        ok,
        "compiled binary did not exit cleanly\nstdout:\n{out}\nstderr:\n{err}"
    );
    assert_eq!(out.trim(), "true");
}
