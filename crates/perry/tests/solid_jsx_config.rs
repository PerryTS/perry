//! Mode selection and cache isolation complement the executable Solid fixture.

use std::path::Path;
use std::process::{Command, Output};

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("temporary project");
    std::fs::write(
        directory.path().join("main.tsx"),
        "console.log(<text>Hello</text>);",
    )
    .expect("JSX entry");
    std::fs::write(
        directory.path().join("host.ts"),
        "export function createElement(name: string) { return { name }; }\n\
         export function spread(node: any, props: any) { node.props = props; }\n",
    )
    .expect("universal host");
    directory
}

fn package(directory: &Path, mode: serde_json::Value) {
    std::fs::write(
        directory.join("package.json"),
        serde_json::json!({
            "type": "module",
            "perry": { "jsx": mode, "packageAliases": { "perry-solid": "./host.ts" } }
        })
        .to_string(),
    )
    .expect("project configuration");
}

fn compile(directory: &Path, name: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(directory)
        .args([
            "compile",
            "main.tsx",
            "--no-link",
            "-o",
            &format!("{name}/output.o"),
        ])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_DISABLE_BUILD_CACHE")
        .output()
        .expect("run Perry")
}

fn objects(path: &Path, output: &mut Vec<Vec<u8>>) {
    if path.is_file() {
        output.push(std::fs::read(path).expect("object bytes"));
    } else {
        for entry in std::fs::read_dir(path).expect("object directory") {
            let path = entry.expect("object entry").path();
            if path.extension().is_some_and(|extension| extension == "o") {
                output.push(std::fs::read(path).expect("object bytes"));
            }
        }
    }
}

fn assert_mode(directory: &Path, name: &str, solid: bool) {
    let result = compile(directory, name);
    assert!(
        result.status.success(),
        "compile failed: {}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let mut bytes = Vec::new();
    objects(&directory.join(name), &mut bytes);
    assert!(!bytes.is_empty(), "the compiler must produce object files");
    let ordinary_jsx = bytes.iter().any(|object| {
        object
            .windows(b"js_jsx".len())
            .any(|window| window == b"js_jsx")
    });
    assert_eq!(
        ordinary_jsx, !solid,
        "the selected mode must reach the correct runtime"
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains(if solid {
            "2 native, 0 JavaScript"
        } else {
            "1 native, 0 JavaScript"
        }),
        "only Solid mode imports the universal host: {stdout}"
    );
}

#[test]
fn mode_switches_preserve_default_jsx_and_do_not_reuse_the_other_object() {
    let directory = fixture();
    package(directory.path(), "default".into());
    assert_mode(directory.path(), "default-first.o", false);
    package(directory.path(), "solid".into());
    assert_mode(directory.path(), "solid-objects", true);
    package(directory.path(), "default".into());
    assert_mode(directory.path(), "default-again.o", false);
    assert_eq!(
        std::fs::read(directory.path().join("default-first.o/output.o")).unwrap(),
        std::fs::read(directory.path().join("default-again.o/output.o")).unwrap(),
        "changing back to default must recover its original object"
    );
}

#[test]
fn toml_mode_overrides_package_mode() {
    let directory = fixture();
    package(directory.path(), "solid".into());
    std::fs::write(
        directory.path().join("perry.toml"),
        "[perry]\njsx = 'default'\n",
    )
    .unwrap();
    assert_mode(directory.path(), "toml-default.o", false);
    package(directory.path(), "default".into());
    std::fs::write(
        directory.path().join("perry.toml"),
        "[perry]\njsx = 'solid'\n",
    )
    .unwrap();
    assert_mode(directory.path(), "toml-solid", true);
}

#[test]
fn invalid_mode_is_diagnosed_before_codegen() {
    let directory = fixture();
    for mode in [
        serde_json::json!("soldi"),
        serde_json::json!(true),
        serde_json::Value::Null,
    ] {
        package(directory.path(), mode);
        let result = compile(directory.path(), "invalid.o");
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("perry.jsx must be"));
    }
}

#[test]
fn tsx_shorthand_compiles_without_an_explicit_subcommand() {
    let directory = fixture();
    package(directory.path(), "solid".into());
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(directory.path())
        .args(["main.tsx", "--no-link", "-o", "shorthand"])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .output()
        .expect("Perry JSX shorthand");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 native, 0 JavaScript"));
}

#[test]
fn a_fragment_of_literals_does_not_import_an_unused_renderer() {
    let directory = fixture();
    package(directory.path(), "solid".into());
    std::fs::write(
        directory.path().join("main.tsx"),
        "console.log(<>{42}{true}</>);",
    )
    .unwrap();
    let result = compile(directory.path(), "fragment");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 native, 0 JavaScript"));
}
