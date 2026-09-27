//! #10433: missing exports fail in preflight, including without a linker.
use std::path::Path;
use std::process::Command;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn compile(main: &str, target: &str, package: bool, expect_missing: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"type":"module","perry":{"compilePackages":["mini"],"allow":{"compilePackages":["mini"]}}}"#,
    );
    write(root, "main.ts", main);
    if package {
        write(
            root,
            "node_modules/mini/package.json",
            r#"{"name":"mini","version":"1.0.0","main":"index.js"}"#,
        );
        write(root, "node_modules/mini/index.js", target);
    } else {
        write(root, "target.ts", target);
        write(root, "target.cjs", target);
    }
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(root)
        .args([
            "compile",
            "main.ts",
            "--no-cache",
            "--no-link",
            "-o",
            "objects",
        ])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .unwrap();
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if expect_missing {
        assert!(!output.status.success(), "{message}");
        assert!(
            message.contains("does not provide an export named 'get'"),
            "{message}"
        );
        assert!(message.contains("main.ts"), "{message}");
        assert!(
            !root.join("objects").exists(),
            "preflight failure must not produce objects"
        );
    } else {
        assert!(output.status.success(), "{message}");
    }
}

#[test]
fn missing_esm_and_cjs_names_are_source_diagnostics() {
    compile(
        "import { get } from './target.ts'; console.log(get());",
        "export function existing() { return 1; }",
        false,
        true,
    );
    let cjs = "var value = function(v) { return v; }; value.get = function(o,p) { return o[p]; }; module.exports = value;";
    compile(
        "import { get } from './target.cjs'; console.log(get({a:1}, 'a'));",
        cjs,
        false,
        true,
    );
    compile(
        "import { get } from 'mini'; console.log(get({a:1}, 'a'));",
        cjs,
        true,
        true,
    );
}

#[test]
fn valid_esm_cjs_default_and_type_imports_still_compile() {
    compile(
        "import { get as read } from './target.ts'; console.log(read());",
        "export function get() { return 1; }",
        false,
        false,
    );
    compile(
        "import { get } from 'mini'; console.log(get());",
        "exports.get = function() { return 1; };",
        true,
        false,
    );
    compile(
        "import mini from 'mini'; console.log(mini.get());",
        "var value = function() {}; value.get = function() { return 1; }; module.exports = value;",
        true,
        false,
    );
    compile(
        "import type { Missing } from './target.ts'; console.log(1);",
        "export const existing = 1;",
        false,
        false,
    );
}
