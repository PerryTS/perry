//! Regression: `require()` of an ES module package that has named exports AND
//! `export default` must yield its namespace object, like Node 22's
//! require(esm). The CJS wrap hoisted `import _req_N from 'pkg'`, so the
//! consumer got the `export default` value: a
//! `require()` of a package with named exports plus `export default fn`
//! returned `fn`, and `(0, mod.named)()` threw "value is not a function".

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn require_of_esm_package_with_default_export_returns_the_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules/esm-pkg");
    std::fs::create_dir_all(package.join("lib")).expect("create package");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "app", "dependencies": { "esm-pkg": "1.0.0" } }"#,
    )
    .expect("write app package.json");
    std::fs::write(
        package.join("package.json"),
        r#"{
  "name": "esm-pkg",
  "type": "module",
  "main": "./lib/index.js",
  "exports": { ".": { "import": "./lib/index.js", "require": "./lib/index.js" } }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        package.join("lib/index.js"),
        "export function getGlobal () { return 'global' }\n\
         export const answer = 42\n\
         export default getGlobal\n",
    )
    .expect("write package");

    let entry = root.join("main.js");
    std::fs::write(
        &entry,
        r#""use strict";
var import_esm = require("esm-pkg");
console.log(typeof import_esm, typeof import_esm.getGlobal, import_esm.answer);
console.log((0, import_esm.getGlobal)(), typeof import_esm.default);
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
        "compiled binary failed\nruntime stdout:\n{}\nruntime stderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "object function 42\nglobal function\n"
    );
}
