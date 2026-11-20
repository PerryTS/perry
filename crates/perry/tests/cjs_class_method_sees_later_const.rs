//! Regression: a class method in a CommonJS module reads a `const`/`let`
//! declared AFTER the class, in a module that also assigns `exports`.
//!
//! The CJS factory lowers the class as a guarded class-environment class: its
//! members read captured bindings from the first evaluation's slot globals.
//! The slots were published when the class was evaluated (the binding still in
//! its dead zone, so `undefined`), and the refresh emitted after the binding's
//! initializer (`RegisterClassCaptures`) skipped guarded classes, so the method
//! kept seeing `undefined` ("later is not a function" for an arrow).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn cjs_class_method_sees_later_const_and_let() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules").join("fake-later-const");
    std::fs::create_dir_all(&package).expect("mkdir package");

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "cjs-later-const",
  "private": true,
  "perry": {
    "compilePackages": ["fake-later-const"],
    "allow": { "compilePackages": ["fake-later-const"] }
  }
}"#,
    )
    .expect("write consumer package.json");
    std::fs::write(
        package.join("package.json"),
        r#"{ "name": "fake-later-const", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write package.json");
    std::fs::write(
        package.join("index.js"),
        r#""use strict";
class P {
  handle(n) { return later(n); }
  value() { return limit; }
  total() { return later(limit); }
}
exports.P = P;
const later = (n) => n + 1;
let limit = 5;
"#,
    )
    .expect("write CJS package");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"import { P } from "fake-later-const";
const p = new P();
console.log(p.handle(1), p.value(), p.total());
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
    assert_eq!(String::from_utf8_lossy(&run.stdout), "2 5 6\n");
}
