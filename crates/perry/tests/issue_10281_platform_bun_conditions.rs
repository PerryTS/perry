//! #10281: `--platform bun` ranks the `bun` condition directly below `perry`
//! for package `exports` and `#imports`; `--platform node` is unchanged.
//!
//! The resolver's unit test flips the platform flag directly. This compiles
//! through the CLI, so it also covers the flag reaching the resolver.

use std::path::{Path, PathBuf};
use std::process::Command;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// A package whose `exports["."]` is `conditions`, shipping one module per
/// condition that exports its own name.
fn package(root: &Path, name: &str, conditions: &str) {
    write(
        root,
        &format!("node_modules/{name}/package.json"),
        &format!(r#"{{"name": "{name}", "type": "module", "exports": {{".": {conditions}}}}}"#),
    );
    for entry in ["perry", "bun", "node", "default"] {
        write(
            root,
            &format!("node_modules/{name}/{entry}.js"),
            &format!("export default \"{name}-{entry}\";\n"),
        );
    }
}

fn compile_and_run(root: &Path, platform: &str) -> String {
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().expect("compiler directory").to_path_buf());
    let output = root.join(format!("main_{platform}"));
    let compile = Command::new(&compiler)
        .current_dir(root)
        .arg("compile")
        .arg(root.join("main.mjs"))
        .arg("--platform")
        .arg(platform)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "--platform {platform} compile failed: {:?}\n{}\n{}",
        compile.status,
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&output)
        .current_dir(root)
        .output()
        .expect("run compiled program");
    assert!(
        run.status.success(),
        "--platform {platform} program failed: {:?}\n{}\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 output")
}

#[test]
fn platform_flag_selects_the_bun_condition_only_for_bun() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "package.json",
        r##"{"name": "bun-condition-probe", "type": "module", "imports": {"#mode": {"bun": "./bun.js", "node": "./node.js", "default": "./default.js"}}}"##,
    );
    for entry in ["bun", "node", "default"] {
        write(
            root,
            &format!("{entry}.js"),
            &format!("export default \"imports-{entry}\";\n"),
        );
    }
    package(
        root,
        "both",
        r#"{"bun": "./bun.js", "node": "./node.js", "default": "./default.js"}"#,
    );
    // An explicit perry entry outranks bun on either target.
    package(
        root,
        "perry-first",
        r#"{"perry": "./perry.js", "bun": "./bun.js", "node": "./node.js"}"#,
    );
    // A package without a bun entry resolves identically on either target.
    package(
        root,
        "no-bun",
        r#"{"node": "./node.js", "default": "./default.js"}"#,
    );
    // A bun entry whose file is missing falls back to the node entry.
    package(
        root,
        "fallback",
        r#"{"bun": "./missing.js", "node": "./node.js"}"#,
    );
    write(
        root,
        "main.mjs",
        "import a from \"both\";\n\
         import b from \"perry-first\";\n\
         import c from \"no-bun\";\n\
         import d from \"fallback\";\n\
         import e from \"#mode\";\n\
         console.log(a, b, c, d, e);\n",
    );

    assert_eq!(
        compile_and_run(root, "node"),
        "both-node perry-first-perry no-bun-node fallback-node imports-node\n"
    );
    assert_eq!(
        compile_and_run(root, "bun"),
        "both-bun perry-first-perry no-bun-node fallback-node imports-bun\n"
    );
}
