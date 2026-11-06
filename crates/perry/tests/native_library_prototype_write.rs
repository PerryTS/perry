//! A foreign FFI body can add an own property that shadows a known holder.
#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::path::Path;
use std::process::{Command, Output};

const WRITER: &str = include_str!("fixtures/native_library_prototype_write/writer.c");
const MAIN: &str = include_str!("fixtures/native_library_prototype_write/main.ts");
const EXPECTED: &str = include_str!("fixtures/native_library_prototype_write/expected.txt");

fn run(mut command: Command, what: &str) -> Output {
    let output = command.output().unwrap_or_else(|e| panic!("{what}: {e}"));
    assert!(
        output.status.success(),
        "{what}: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn ffi_own_store_shadows_an_inherited_value() {
    for tool in ["clang", "ar"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("SKIP: {tool} unavailable");
            return;
        }
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules/foreign-writer");
    std::fs::create_dir_all(package.join("src")).unwrap();
    std::fs::write(package.join("writer.c"), WRITER).unwrap();
    run(
        {
            let mut command = Command::new("clang");
            command
                .current_dir(&package)
                .args(["-c", "-fPIC", "writer.c", "-o", "writer.o"]);
            command
        },
        "build foreign writer",
    );
    run(
        {
            let mut command = Command::new("ar");
            command
                .current_dir(&package)
                .args(["rcs", "libwriter.a", "writer.o"]);
            command
        },
        "archive foreign writer",
    );
    std::fs::write(
        package.join("src/index.ts"),
        "export declare function js_cxnapi_write(object: any, key: any, value: any): void;\n",
    )
    .unwrap();
    std::fs::write(package.join("package.json"), r#"{
      "name": "foreign-writer", "version": "1.0.0",
      "main": "src/index.ts", "types": "src/index.ts",
      "perry": { "nativeLibrary": {
        "abiVersion": "0.5",
        "functions": [{ "name": "js_cxnapi_write", "params": ["jsvalue", "jsvalue", "jsvalue"], "returns": "void" }],
        "targets": {
          "linux": { "prebuilt": "./libwriter.a" },
          "macos": { "prebuilt": "./libwriter.a" }
        }
      }}
    }"#).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{
      "name": "foreign-writer-host", "private": true,
      "perry": { "allow": { "nativeLibrary": ["foreign-writer"] } }
    }"#,
    )
    .unwrap();
    std::fs::write(root.join("main.ts"), MAIN).unwrap();
    let app = root.join("app");
    run(
        {
            let mut command = Command::new(env!("CARGO_BIN_EXE_perry"));
            command
                .current_dir(root)
                .env(
                    "PERRY_WORKSPACE_ROOT",
                    Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
                )
                .arg("compile")
                .arg("main.ts")
                .arg("--no-cache")
                .arg("-o")
                .arg(&app);
            command
        },
        "compile FFI prototype writer",
    );
    let output = run(Command::new(&app), "run FFI prototype writer");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        EXPECTED,
        "the compiled holder read must observe the foreign own store"
    );
}
