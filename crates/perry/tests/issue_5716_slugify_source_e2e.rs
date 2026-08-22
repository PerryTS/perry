//! #5716: ordinary npm packages should compile from their installed source,
//! not route through an in-tree Rust rewrite.
//!
//! This test installs the last version covered by Perry's former slugify shim,
//! compiles the real CommonJS package through the default `compilePackages:
//! auto` path, and compares its observable output with Node. Set
//! `PERRY_REQUIRE_NPM_E2E=1` to make an unavailable npm/network a hard failure.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

fn ensure_runtime_archives() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static")
            .output()
            .expect("build Perry runtime archives");
        assert_success("runtime archive build", &build);
    });
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn npm_install(root: &Path) -> bool {
    let output = Command::new("npm")
        .current_dir(root)
        .arg("install")
        .arg("--no-audit")
        .arg("--no-fund")
        .output();
    let required = std::env::var("PERRY_REQUIRE_NPM_E2E").ok().as_deref() == Some("1");
    match output {
        Ok(output) if output.status.success() => true,
        Ok(output) if required => {
            assert_success("npm install slugify@1.6.9", &output);
            false
        }
        Ok(output) => {
            eprintln!(
                "SKIP: npm install slugify@1.6.9 failed (offline?)\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            false
        }
        Err(error) if required => panic!("npm is required for slugify source E2E: {error}"),
        Err(error) => {
            eprintln!("SKIP: npm is unavailable: {error}");
            false
        }
    }
}

#[test]
fn installed_slugify_source_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "perry-issue-5716-slugify",
  "private": true,
  "type": "module",
  "dependencies": { "slugify": "1.6.9" }
}"#,
    )
    .expect("write package.json");
    if !npm_install(root) {
        return;
    }

    std::fs::write(
        root.join("main.ts"),
        r#"import slugify from "slugify";

const outputs = [
  slugify("some string"),
  slugify("Déjà Vu!"),
  slugify("foo_bar baz", {
    replacement: "_",
    lower: true,
    strict: true,
    trim: true,
  }),
  slugify("Äpfel & Öl", { lower: true, locale: "de" }),
  slugify("a*b+c", { remove: /[*+]/g }),
  typeof slugify.extend,
];

slugify.extend({ "☢": "radioactive", "♥": "love" });
outputs.push(slugify("☢ ♥", { lower: true }));

for (const output of outputs) console.log(output);
"#,
    )
    .expect("write source probe");

    let node = Command::new("node")
        .current_dir(root)
        .arg("--experimental-strip-types")
        .arg("main.ts")
        .output()
        .expect("run source probe with Node");
    assert_success("Node slugify probe", &node);

    ensure_runtime_archives();
    let binary = root.join("perry-out");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg("main.ts")
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--verbose")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir())
        .output()
        .expect("compile installed slugify source");
    assert_success("Perry slugify source compile", &compile);

    let compile_log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(
        !compile_log.contains("failed to compile") && !compile_log.contains("empty stubs"),
        "source migration must not hide failed package modules:\n{compile_log}"
    );
    assert!(
        compile_log.contains("Compile package wildcard: expanded to 1 installed package(s)"),
        "default auto routing must select the installed package:\n{compile_log}"
    );
    assert!(
        !compile_log.contains("perry_ext_slugify"),
        "removed native archive must not appear on the link line:\n{compile_log}"
    );

    let perry = Command::new(&binary)
        .current_dir(root)
        .output()
        .expect("run Perry slugify probe");
    assert_success("Perry slugify probe", &perry);
    assert_eq!(
        perry.stdout, node.stdout,
        "installed slugify source must match Node byte-for-byte"
    );
    assert!(
        perry.stderr.is_empty(),
        "Perry slugify probe wrote stderr:\n{}",
        String::from_utf8_lossy(&perry.stderr)
    );
}
