//! `require("./" + x)` inside a compiled package globs the package directory.
//! The candidates follow node's resolution (extension probing, directories),
//! and a candidate that is not JavaScript (LICENSE) only throws the
//! `SyntaxError` when required, as in node, instead of failing the compile.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn computed_require_glob_follows_node_and_defers_unparsable_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules/globpkg");
    std::fs::create_dir_all(package.join("sub")).unwrap();
    std::fs::create_dir_all(package.join("pj")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"type":"module","perry":{"compilePackages":["globpkg"],"allow":{"compilePackages":["globpkg"]}}}"#,
    )
    .unwrap();
    for (file, source) in [
        (
            "package.json",
            r#"{"name":"globpkg","version":"1.0.0","main":"index.js"}"#,
        ),
        ("LICENSE", "MIT License\n\nCopyright (c) someone\n"),
        ("README", "# readme (not javascript\n"),
        ("plain", "module.exports = { kind: 'extensionless' };\n"),
        ("a.js", "module.exports = { kind: 'js' };\n"),
        ("b.json", "{ \"kind\": \"json\" }\n"),
        ("sub/index.js", "module.exports = { kind: 'dir-index' };\n"),
        ("pj/entry.js", "module.exports = { kind: 'dir-main' };\n"),
        ("pj/package.json", r#"{"main":"entry.js"}"#),
        (
            "index.js",
            "export function load(e) { return require('./' + e); }\n",
        ),
    ] {
        std::fs::write(package.join(file), source).unwrap();
    }
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { load } from 'globpkg';
const names = ['plain', 'a.js', 'a', 'b.json', 'b', 'sub', 'pj', 'LICENSE', 'README', 'missing.js'];
for (const x of names) {
  try { const m: any = load(x); console.log(x, 'ok', JSON.stringify(m)); }
  catch (e: any) { console.log(x, 'threw', e instanceof SyntaxError ? 'SyntaxError' : e.code || e.name); }
}
"#,
    )
    .unwrap();

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
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
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "plain ok {\"kind\":\"extensionless\"}\n\
         a.js ok {\"kind\":\"js\"}\n\
         a ok {\"kind\":\"js\"}\n\
         b.json ok {\"kind\":\"json\"}\n\
         b ok {\"kind\":\"json\"}\n\
         sub ok {\"kind\":\"dir-index\"}\n\
         pj ok {\"kind\":\"dir-main\"}\n\
         LICENSE threw SyntaxError\n\
         README threw SyntaxError\n\
         missing.js threw MODULE_NOT_FOUND\n",
        "stderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
}
