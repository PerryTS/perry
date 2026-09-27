//! #10436: unwrapped package TS must resolve require relative to its module.
use std::process::{Command, Output};

fn success(command: &mut Command) -> Output {
    let output = command.output().expect("run command");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn package_require_ignores_cwd_and_preserves_cache_and_shadowing() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let pkg = root.join("node_modules/relative-lib");
    let source = pkg.join("src");
    let lib = pkg.join("lib");
    let decoy = root.join("decoy");
    for path in [&source, &lib, &decoy] {
        std::fs::create_dir_all(path).unwrap();
    }
    std::fs::write(root.join("package.json"), r#"{
        "type":"module", "perry":{"compilePackages":["relative-lib"],"allow":{"compilePackages":["*"]}}
    }"#).unwrap();
    std::fs::write(
        pkg.join("package.json"),
        r#"{"name":"relative-lib","version":"1.0.0","main":"lib/index.js"}"#,
    )
    .unwrap();
    let body = r#"
const direct = require('./helper.cjs');
const json = require('./data.json');
const read = require;
function probe() {
  const key = read.resolve('./helper.cjs');
  console.log('values', direct.value, json.value, read('./data.json').value);
  console.log('cache', read('./helper.cjs') === direct, read.cache[key].exports === direct);
}
function shadowed(require) { return require('x'); }
"#;
    std::fs::write(
        source.join("index.ts"),
        format!("{body}\nexport {{ probe, shadowed }};\n"),
    )
    .unwrap();
    std::fs::write(
        lib.join("index.js"),
        format!("{body}\nexports.probe = probe; exports.shadowed = shadowed;\n"),
    )
    .unwrap();
    for path in [&source, &lib] {
        std::fs::write(
            path.join("helper.cjs"),
            "module.exports = {value:'module-cjs'};",
        )
        .unwrap();
        std::fs::write(path.join("data.json"), r#"{"value":"module-json"}"#).unwrap();
    }
    // Both possible cwd lookups succeed with wrong values, rather than merely
    // failing because the files happen to be absent.
    for path in [&root, &decoy] {
        std::fs::write(
            path.join("helper.cjs"),
            "module.exports = {value:'wrong-cwd'};",
        )
        .unwrap();
        std::fs::write(path.join("data.json"), r#"{"value":"wrong-cwd"}"#).unwrap();
    }
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { probe, shadowed } from 'relative-lib';
probe();
console.log(shadowed((value: string) => 'shadow:' + value));
"#,
    )
    .unwrap();
    let expected = b"values module-cjs module-json module-json\ncache true true\nshadow:x\n";
    for cwd in [&root, &source, &decoy] {
        let node = success(Command::new("node").arg(&entry).current_dir(cwd));
        assert_eq!(node.stdout, expected);
    }
    let binary = root.join(if cfg!(windows) { "app.exe" } else { "app" });
    success(
        Command::new(env!("CARGO_BIN_EXE_perry"))
            .current_dir(&root)
            .args(["compile", "--no-cache", "--no-auto-optimize"])
            .arg(&entry)
            .arg("-o")
            .arg(&binary),
    );
    for cwd in [&root, &source, &decoy] {
        let native = success(Command::new(&binary).current_dir(cwd));
        assert_eq!(native.stdout, expected, "cwd: {}", cwd.display());
    }
}
