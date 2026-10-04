//! Regression test: a module-level function named like a builtin global
//! (`function process(parentNode) {}`) was called as `process.call(this, x)`
//! and silently returned `undefined`.
//!
//! `member_tail.rs` rewrites `X.bind` / `X.call` / `X.apply` /
//! `X.isPrototypeOf` to `globalThis.X.<m>` when `X` names a builtin (#4533 /
//! #4561, so `Error.isPrototypeOf(x)` reads the real constructor). It did so
//! without asking whether a local binding shadows the name, replacing the
//! correctly lowered receiver -- the local function -- with Node's `process`
//! object, whose `call` is undefined.
//!
//! turndown 7.2.0 has exactly this: `var output = process.call(this, root)` in
//! its `turndown()` method, so every HTML-to-Markdown conversion handed
//! `postProcess` an `undefined` and threw "Cannot read properties of undefined
//! (reading 'length')" -- OpenCode's `webfetch` tool.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

const LIB_SOURCE: &str = r#"
// turndown's shape: a module-level function named like a Node global, called
// through Function.prototype.call from an object-literal method.
function process (parentNode) { return "processed:" + parentNode }
function postProcess (output) { return output + "|post" }
function Buffer (n) { return "buf" + n }
var service = {
  turndown: function (input) {
    var output = process.call(this, input);
    return postProcess.call(this, output)
  }
};
export function run () {
  var out = [];
  out.push("1 direct: " + process("a"));
  out.push("2 call: " + process.call(null, "b"));
  out.push("3 apply: " + process.apply(null, ["c"]));
  out.push("4 bind: " + process.bind(null, "d")());
  out.push("5 turndown shape: " + service.turndown("<p>"));
  out.push("6 another shadowed global: " + Buffer.call(null, 7));
  return out.join("\n");
}
"#;

const MAIN_SOURCE: &str = r#"
import { run } from "./lib.js"
console.log(run())
// Controls (#4533/#4561): the unshadowed builtins keep resolving to the real constructors.
console.log("7 Error.isPrototypeOf:", Error.isPrototypeOf(TypeError))
console.log("8 Number.bind:", Number.bind(null, "42")())
console.log("9 global process:", typeof process.cwd)
"#;

/// Byte-for-byte what node 26.5.1 prints.
const EXPECTED: &str = "\
1 direct: processed:a
2 call: processed:b
3 apply: processed:c
4 bind: processed:d
5 turndown shape: processed:<p>|post
6 another shadowed global: buf7
7 Error.isPrototypeOf: true
8 Number.bind: 42
9 global process: function
";

#[test]
fn a_shadowed_builtin_name_keeps_its_own_call_and_apply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("lib.js"), LIB_SOURCE).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_SOURCE).unwrap();

    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "shadowed-global probe must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary must run; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8(run.stdout).expect("UTF-8 stdout");
    assert_eq!(
        stdout, EXPECTED,
        "a local binding named like a builtin global must keep its own call/apply/bind"
    );
}
