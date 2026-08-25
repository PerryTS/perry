//! Regression test for #8749: a Node builtin named import used from a
//! `compilePackages` dependency must retain its runtime binding.
//!
//! `@hono/node-server` imports `createServer` from `http`, selects it through a
//! module-scope fallback (`options.createServer || createServerHTTP`), and calls
//! the selected function later from `serve()`. App-level imports already
//! worked; the binding was lost specifically while compiling the dependency.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn builtin_named_import_survives_module_scope_fallback_in_compiled_package() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "compiled-builtin-import-consumer",
  "private": true,
  "type": "module",
  "perry": {
    "compilePackages": ["fake-node-server"],
    "allow": { "compilePackages": ["fake-node-server"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("fake-node-server");
    std::fs::create_dir_all(&pkg).expect("mkdir fake-node-server");
    std::fs::write(
        pkg.join("package.json"),
        r#"{
  "name": "fake-node-server",
  "version": "1.0.0",
  "type": "module",
  "exports": "./index.mjs"
}"#,
    )
    .expect("write dependency package.json");
    std::fs::write(
        pkg.join("index.mjs"),
        r#"
import { createServer as createServerHTTP } from "http";
import { createServer as createServerNodeHTTP } from "node:http";

const options = {};
const selectedHTTP = options.createServer || createServerHTTP;
const selectedNodeHTTP = options.createServer || createServerNodeHTTP;

export function inspectBindings() {
  const serverHTTP = selectedHTTP({}, () => {});
  const serverNodeHTTP = selectedNodeHTTP({}, () => {});
  return [
    typeof createServerHTTP,
    typeof createServerNodeHTTP,
    typeof selectedHTTP,
    typeof selectedNodeHTTP,
    typeof serverHTTP.listen,
    typeof serverNodeHTTP.listen,
  ].join(",");
}
"#,
    )
    .expect("write compiled dependency");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { inspectBindings } from "fake-node-server";
console.log(inspectBindings());
process.exit(0);
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
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
        "function,function,function,function,function,function\n",
        "both builtin spellings must stay bound through the dependency's module global"
    );
}
