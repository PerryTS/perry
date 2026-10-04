//! A module reached only through a function-local `require` must load on a
//! Worker thread as it does on the main thread.
//!
//! Such a target is Deferred: the entry's `main` registers its initializer
//! address by path and the first runtime `require` runs it. That table is
//! per-thread (one per heap, so several applications can share a process),
//! and a Worker never runs `main` — it runs only its entry's `__init`. Its
//! table was therefore empty, and the same `require` that worked on the main
//! thread threw `MODULE_NOT_FOUND` on the worker. This is turndown's shape
//! (`createHTMLParser` requires `@mixmark-io/domino`, a package whose `main`
//! is a directory), and it killed OpenCode's TUI worker at startup.
//!
//! The main thread loads the same module first as the control: it must keep
//! working, and the worker must print the same value.

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

/// A package whose `main` names a directory, like `@mixmark-io/domino`.
const DOM_PACKAGE_JSON: &str = r#"{ "name": "fakedom", "main": "./lib" }"#;
const DOM_INDEX: &str = r#"
exports.parse = function (s) { return "parsed:" + s; };
"#;

/// turndown's shape: an ES module in an installed package whose `require`
/// sits inside a function called at module init, so the target is Deferred
/// rather than initialized with the static graph.
const PARSER_PACKAGE_JSON: &str = r#"{ "name": "fakeparser", "main": "lib/parser.es.js" }"#;
const PARSER_SOURCE: &str = r#"
function createParser() {
  var Dom = require("fakedom");
  return Dom.parse;
}
var parse = createParser();
export default parse;
"#;

const WORKER_SOURCE: &str = r#"
// @ts-nocheck
import parse from "fakeparser"
postMessage("worker " + parse("x"))
"#;

const MAIN_SOURCE: &str = r#"
// @ts-nocheck
import parse from "fakeparser"
console.log("main " + parse("x"))
const worker = new Worker("./worker.ts", { type: "module" })
worker.onmessage = (e) => { console.log(e.data); process.exit(0) }
setTimeout(() => {
  console.log("TIMEOUT waiting for the worker")
  process.exit(1)
}, 15000)
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "main parsed:x\nworker parsed:x\n";

#[test]
fn worker_loads_a_module_reached_only_by_a_function_local_require() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let modules = root.join("node_modules");
    let dom = modules.join("fakedom");
    std::fs::create_dir_all(dom.join("lib")).unwrap();
    std::fs::write(dom.join("package.json"), DOM_PACKAGE_JSON).unwrap();
    std::fs::write(dom.join("lib").join("index.js"), DOM_INDEX).unwrap();
    let parser = modules.join("fakeparser");
    std::fs::create_dir_all(parser.join("lib")).unwrap();
    std::fs::write(parser.join("package.json"), PARSER_PACKAGE_JSON).unwrap();
    std::fs::write(parser.join("lib").join("parser.es.js"), PARSER_SOURCE).unwrap();
    std::fs::write(root.join("worker.ts"), WORKER_SOURCE).unwrap();
    std::fs::write(root.join("main.ts"), MAIN_SOURCE).unwrap();

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "pre-fix the worker's require threw MODULE_NOT_FOUND\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, EXPECTED,
        "the worker must load the module the main thread loads"
    );
}
