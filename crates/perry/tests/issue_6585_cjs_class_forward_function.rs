//! Regression for #6585: a class method in a CommonJS module can call a
//! module-scoped function declaration that appears later in source order.
//! Ajv's `dist/compile/codegen/index.js` uses this exact shape for `addNames`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn cjs_class_method_sees_later_function_declaration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules").join("fake-ajv-codegen");
    std::fs::create_dir_all(&package).expect("mkdir package");

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-6585",
  "private": true,
  "perry": {
    "compilePackages": ["fake-ajv-codegen"],
    "allow": { "compilePackages": ["fake-ajv-codegen"] }
  }
}"#,
    )
    .expect("write consumer package.json");
    std::fs::write(
        package.join("package.json"),
        r#"{ "name": "fake-ajv-codegen", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write package.json");
    std::fs::write(
        package.join("index.js"),
        r#""use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.CodeGen = void 0;

class ParentNode {
  constructor(nodes) {
    this.nodes = nodes;
  }
  get names() {
    return this.nodes.reduce((names, node) => addNames(names, node.names), {});
  }
}

class CodeGen {
  run() {
    return new ParentNode([{ names: { alpha: 1 } }, { names: { alpha: 2, beta: 4 } }]).names;
  }
}
exports.CodeGen = CodeGen;

function addNames(names, from) {
  for (const name in from) names[name] = (names[name] || 0) + (from[name] || 0);
  return names;
}
"#,
    )
    .expect("write CJS package");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { CodeGen } from "fake-ajv-codegen";
console.log(JSON.stringify(new CodeGen().run()));
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
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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
        "compiled binary failed\ncompile stdout:\n{}\nruntime stdout:\n{}\nruntime stderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "{\"alpha\":3,\"beta\":4}\n"
    );
}
