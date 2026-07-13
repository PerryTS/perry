//! Characterization coverage for variable-shaped exports reached through a
//! namespace binding.  The export ABI is a zero-argument getter returning the
//! closure, not a function symbol with the closure's user-facing arity.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn namespace_variable_exports_use_their_getter_then_call_the_closure() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "namespace-variable-export-abi",
  "type": "module",
  "perry": {
    "compilePackages": ["mini-cjs"],
    "allow": { "compilePackages": ["mini-cjs"] }
  }
}"#,
    )
    .expect("write package manifest");

    std::fs::write(
        root.join("vars.ts"),
        r#"
export const make = (value: string) => "make:" + value;
export const alsoMake = (value: string) => "also:" + value;
export { alsoMake as aliasedMake };
export function declared(value: string) { return "declared:" + value; }
export default (value: string) => "default:" + value;
"#,
    )
    .expect("write vars");
    std::fs::write(
        root.join("barrel.ts"),
        r#"export * as Reexported from "./vars.js";"#,
    )
    .expect("write barrel");
    // The cycle must retain module-init ordering: evaluating the imported
    // closure is deferred until after both modules finish initialization.
    std::fs::write(
        root.join("cycle-a.ts"),
        r#"
import * as CycleB from "./cycle-b.js";
export const make = (value: string) => CycleB.prefix(value) + ":a";
export const ready = () => "ready";
"#,
    )
    .expect("write cycle-a");
    std::fs::write(
        root.join("cycle-b.ts"),
        r#"
import * as CycleA from "./cycle-a.js";
export const prefix = (value: string) => "b:" + value;
export const readReady = () => CycleA.ready();
"#,
    )
    .expect("write cycle-b");
    let cjs = root.join("node_modules/mini-cjs");
    std::fs::create_dir_all(&cjs).expect("create cjs package");
    std::fs::write(
        cjs.join("package.json"),
        r#"{ "name": "mini-cjs", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write cjs manifest");
    std::fs::write(
        cjs.join("index.js"),
        "var make = require('./make'); module.exports = { make: make };\n",
    )
    .expect("write cjs barrel");
    std::fs::write(
        cjs.join("make.js"),
        "module.exports = function (value) { return 'cjs:' + value; };\n",
    )
    .expect("write cjs function");
    std::fs::write(
        root.join("main.ts"),
        r#"
import * as API from "./vars.js";
import { Reexported } from "./barrel.js";
import * as CycleA from "./cycle-a.js";
import * as CycleB from "./cycle-b.js";
import * as CJS from "mini-cjs";

console.log(API.make("one"));
console.log(API.aliasedMake("two"));
console.log(API.declared("three"));
console.log(API.default("four"));
console.log(Reexported.make("five"));
console.log(CycleA.make("six"));
console.log(CycleB.readReady());
console.log(CJS.make("seven"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "make:one\nalso:two\ndeclared:three\ndefault:four\nmake:five\nb:six:a\nready\ncjs:seven\n"
    );
}
