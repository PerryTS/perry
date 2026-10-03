//! Regression test: an object literal whose key is the empty string,
//! `{ "": v }`, aborted at the literal with "refusing to publish invalid
//! object shape facts".
//!
//! Codegen packs key names as `name\0` per name, so the key `""` is the
//! single byte `\0`. Every runtime reader split the packed names on NUL and
//! dropped empty segments, so the empty key vanished: the keys array came out
//! shorter than its count and the shape mint refused the facts. Across two
//! modules it failed differently: both literals are one content, so they
//! share one static ShapeId, and each module built its own empty array --
//! different facts under one id, and the second module's mint was refused.
//! That is how the natively compiled OpenCode v1.18.30 TUI died at startup:
//! json5's `parse.js` builds the JSON reviver holder `{ '': root }`.
//!
//! The literal lives in two modules here so the static-id path is exercised,
//! and once inside a function so it is born at a call, not only at init.

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

const OTHER_SOURCE: &str = r#"
export const otherHolder = { "": "second module" }
export function wrap(v: unknown) {
  return { "": v }
}
"#;

const MAIN_SOURCE: &str = r#"
import { otherHolder, wrap } from "./other.js"

const holder = { "": 42 }
console.log("1 holder[\"\"]:", holder[""])
console.log("2 keys:", JSON.stringify(Object.keys(holder)))
console.log("3 in:", "" in holder)
const mixed = { "": 1, a: 2 }
console.log("4 mixed:", JSON.stringify(mixed), mixed[""], mixed.a)
console.log("5 reviver:", JSON.stringify(JSON.parse("{\"x\":1}", (k, v) => (k === "" ? { root: v } : v))))
console.log("6 other module:", otherHolder[""])
console.log("7 built in a function:", JSON.stringify(wrap([1, 2])))
"#;

/// Byte-for-byte what node 26.5.1 prints.
const EXPECTED: &str = "\
1 holder[\"\"]: 42
2 keys: [\"\"]
3 in: true
4 mixed: {\"\":1,\"a\":2} 1 2
5 reviver: {\"root\":{\"x\":1}}
6 other module: second module
7 built in a function: {\"\":[1,2]}
";

#[test]
fn an_empty_string_key_survives_an_object_literal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("other.ts"), OTHER_SOURCE).unwrap();
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
        "empty-key probe must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary must run, not abort at the literal; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8(run.stdout).expect("UTF-8 stdout");
    assert_eq!(stdout, EXPECTED, "the key `\"\"` is a key like any other");
}
