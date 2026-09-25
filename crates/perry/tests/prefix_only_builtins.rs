//! Prefix-only Node builtins must not consume same-named npm packages.
use std::path::Path;
use std::process::{Command, Output};

fn compile(dir: &Path, name: &str, source: &str) -> Output {
    let entry = dir.join(name);
    std::fs::write(&entry, source).unwrap();
    Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir)
        .args(["compile", "--no-cache", "--no-auto-optimize"])
        .arg(entry)
        .arg("-o")
        .arg(dir.join("app"))
        .output()
        .unwrap()
}

fn run(dir: &Path, name: &str, source: &str) -> String {
    let result = compile(dir, name, source);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(dir.join(if cfg!(windows) { "app.exe" } else { "app" }))
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout)
        .unwrap()
        .replace("\r\n", "\n")
}

fn packages(dir: &Path) {
    std::fs::write(dir.join("package.json"), r#"{"type":"module","perry":{"compilePackages":["sea","sqlite","test"],"allow":{"compilePackages":["*"]}}}"#).unwrap();
    for name in ["sea", "sqlite", "test"] {
        let package = dir.join("node_modules").join(name);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            format!(r#"{{"name":"{name}","version":"1.0.0","main":"index.cjs"}}"#),
        )
        .unwrap();
        std::fs::write(
            package.join("index.cjs"),
            format!("exports.marker = '{name}';\n"),
        )
        .unwrap();
    }
    let test = dir.join("node_modules/test");
    std::fs::write(test.join("package.json"), r#"{"name":"test","version":"1.0.0","main":"index.cjs","exports":{".":"./index.cjs","./reporters":"./reporters.cjs"}}"#).unwrap();
    std::fs::write(
        test.join("reporters.cjs"),
        "exports.marker = 'test/reporters';\n",
    )
    .unwrap();
}

#[test]
fn missing_bare_prefix_only_modules_do_not_resolve_as_builtins() {
    for name in ["sea", "sqlite", "test", "test/reporters"] {
        let dir = tempfile::tempdir().unwrap();
        let output = compile(
            dir.path(),
            "main.ts",
            &format!("import * as value from '{name}'; console.log(typeof value);"),
        );
        assert!(
            !output.status.success(),
            "uninstalled package {name} compiled as a builtin"
        );
    }
}

#[test]
fn esm_packages_coexist_with_explicit_node_builtins() {
    let dir = tempfile::tempdir().unwrap();
    packages(dir.path());
    let stdout = run(
        dir.path(),
        "main.ts",
        r#"
        import sea from 'sea';
        import sqlite from 'sqlite';
        import * as test from 'test';
        import { marker as reporters } from 'test/reporters';
        import * as nodeSea from 'node:sea';
        import * as nodeSqlite from 'node:sqlite';
        import * as nodeTest from 'node:test';
        import * as nodeReporters from 'node:test/reporters';
        console.log(sea.marker, sqlite.marker, test.marker, reporters);
        console.log(typeof nodeSea, typeof nodeSqlite, typeof nodeTest, typeof nodeReporters);
        console.log(typeof nodeSea.isSea, typeof nodeSqlite.DatabaseSync, typeof nodeTest.test, typeof nodeReporters.spec);
    "#,
    );
    assert_eq!(stdout, "sea sqlite test test/reporters\nobject object object object\nfunction function function function\n");
}

#[test]
fn commonjs_requires_resolve_same_named_packages() {
    let dir = tempfile::tempdir().unwrap();
    packages(dir.path());
    let stdout = run(
        dir.path(),
        "main.cjs",
        r#"
        const sea = require('sea'), sqlite = require('sqlite');
        const test = require('test'), reporters = require('test/reporters');
        console.log(sea.marker, sqlite.marker, test.marker, reporters.marker);
    "#,
    );
    assert_eq!(stdout, "sea sqlite test test/reporters\n");
}

#[test]
fn namespace_reexports_keep_packages_and_builtins_distinct() {
    let dir = tempfile::tempdir().unwrap();
    packages(dir.path());
    std::fs::write(
        dir.path().join("barrel.ts"),
        r#"
        export * as sea from 'sea';
        export * as sqlite from 'sqlite';
        export * as test from 'test';
        export * as reporters from 'test/reporters';
        export * as nativeSea from 'node:sea';
        export { DatabaseSync } from 'node:sqlite';
    "#,
    )
    .unwrap();
    let stdout = run(
        dir.path(),
        "main.ts",
        r#"
        import {sea, sqlite, test, reporters, nativeSea, DatabaseSync} from './barrel.ts';
        console.log(sea.marker, sqlite.marker, test.marker, reporters.marker);
        console.log(typeof nativeSea.isSea, typeof DatabaseSync);
    "#,
    );
    assert_eq!(
        stdout,
        "sea sqlite test test/reporters\nfunction function\n"
    );
}

#[test]
fn optional_missing_prefix_only_names_do_not_resolve_as_builtins() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = String::new();
    for name in ["sea", "sqlite", "test", "test/reporters"] {
        source.push_str(&format!(
            "try {{ console.log(typeof require('{name}')); }} catch (e) {{ console.log(e.code); }}\ntry {{ console.log(require.resolve('{name}')); }} catch (e) {{ console.log(e.code); }}\n"
        ));
    }
    source.push_str("let name = 'sea'; try { console.log(typeof require(name)); } catch (e) { console.log(e.code); }\n");
    let stdout = run(dir.path(), "main.cjs", &source);
    assert_eq!(stdout, "MODULE_NOT_FOUND\n".repeat(9));
}
