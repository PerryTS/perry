//! #10078: local macOS UI builds need a real application bundle. This exercises
//! linking, packaging, and signing without launching a window in the test runner.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn checked(mut command: Command) -> Output {
    let output = command.output().expect("run command");
    assert!(
        output.status.success(),
        "{command:?}: {}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn runtime_dir() -> PathBuf {
    let archives = [
        "libperry_runtime.a",
        "libperry_stdlib.a",
        "libperry_ext_net.a",
        "libperry_ui_macos.a",
    ];
    if let Some(dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        let dir = PathBuf::from(dir);
        if archives.iter().all(|name| dir.join(name).is_file()) {
            return dir;
        }
    }
    // Build one coherent archive set when the caller has not supplied one.
    let mut build = Command::new(env!("CARGO"));
    build
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "build",
            "--profile",
            "perry-dev",
            "--message-format=json",
            "-p",
            "perry-runtime-static",
            "-p",
            "perry-stdlib-static",
            "-p",
            "perry-ext-net",
            "-p",
            "perry-ui-macos",
        ]);
    let output = checked(build);
    let dir = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|item| item["reason"] == "compiler-artifact")
        .filter_map(|item| item["filenames"].as_array().cloned())
        .flatten()
        .filter_map(|file| file.as_str().map(PathBuf::from))
        .find(|file| file.file_name().is_some_and(|name| name == archives[0]))
        .expect("runtime archive in Cargo output")
        .parent()
        .unwrap()
        .to_path_buf();
    assert!(archives.iter().all(|name| dir.join(name).is_file()));
    dir
}

fn compile(root: &Path, runtime: &Path, source: &str, output: &str) -> PathBuf {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perry"));
    command
        .current_dir(root)
        .env("PERRY_RUNTIME_DIR", runtime)
        .env("PERRY_LL_OPT_LEVEL", "0")
        .env_remove("PERRY_KEEP_SYMBOLS")
        .env_remove("PERRY_DEBUG_SYMBOLS")
        .args([
            "--format",
            "json",
            "compile",
            source,
            "-o",
            output,
            "--no-cache",
            "--no-auto-optimize",
            "--no-codegen",
            "--emit-attest",
            "--emit-sandbox",
        ]);
    let output = checked(command);
    let result = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|item| item["success"] == true && item["output"].is_string())
        .expect("successful compilation JSON");
    root.join(result["output"].as_str().unwrap())
}

fn verify_bundle(app: &Path, expected_name: &str, expected_version: &str) {
    let mut plutil = Command::new("/usr/bin/plutil");
    plutil
        .args(["-convert", "json", "-o", "-"])
        .arg(app.join("Contents/Info.plist"));
    let plist: serde_json::Value = serde_json::from_slice(&checked(plutil).stdout).unwrap();
    assert_eq!(plist["CFBundleIdentifier"], "dev.perry.bundle10078");
    assert_eq!(plist["CFBundleName"], expected_name);
    assert_eq!(plist["CFBundleDisplayName"], expected_name);
    assert_eq!(plist["CFBundleShortVersionString"], expected_version);
    assert_eq!(plist["CFBundleVersion"], "7");
    assert_eq!(plist["CFBundleIconFile"], "AppIcon.icns");
    assert_eq!(
        std::fs::read(app.join("Contents/Resources/AppIcon.icns")).unwrap(),
        b"icns\0\0\0\x08"
    );
    assert_eq!(plist["CFBundlePackageType"], "APPL");
    assert!(plist["NSCameraUsageDescription"].is_string());
    assert!(plist["NSMicrophoneUsageDescription"].is_string());
    let executable = app
        .join("Contents/MacOS")
        .join(plist["CFBundleExecutable"].as_str().unwrap());
    assert!(executable.is_file());
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/Resources/assets/message.txt")).unwrap(),
        "bundle asset\n"
    );
    assert!(app.with_extension("app.sandbox").is_file());
    assert!(!executable.with_extension("sandbox").exists());
    assert!(!executable.with_extension("attest.json").exists());
    let attestation: serde_json::Value =
        serde_json::from_slice(&std::fs::read(app.with_extension("app.attest.json")).unwrap())
            .unwrap();
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(&executable).unwrap();
    assert_eq!(
        attestation["sha256"],
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    assert_eq!(attestation["size"], bytes.len() as u64);
    let mut verify = Command::new("/usr/bin/codesign");
    verify.args(["--verify", "--deep", "--strict"]).arg(app);
    checked(verify);
}

#[test]
fn ui_outputs_are_signed_bundles_with_resources_and_final_binary_attestations() {
    let runtime = runtime_dir();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("assets")).unwrap();
    std::fs::write(root.join("assets/message.txt"), "bundle asset\n").unwrap();
    std::fs::write(root.join("assets/AppIcon.icns"), b"icns\0\0\0\x08").unwrap();
    std::fs::write(root.join("package.json"), r#"{"type":"module"}"#).unwrap();
    std::fs::write(
        root.join("perry.toml"),
        r#"
[project]
name = "bundle-test"
version = "2.3.4"
build_number = 7
[macos]
bundle_id = "dev.perry.bundle10078"
display_name = "Perry & Bundle"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("main.ts"),
        r#"
import { App, Text } from "perry/ui";
App({ title: "Bundle test", body: Text("Bundle test") });
"#,
    )
    .unwrap();
    let app = compile(root, &runtime, "main.ts", "My App.v2");
    assert_eq!(app, root.join("My App.v2.app"));
    assert!(
        root.join("My App.v2").is_file(),
        "keep the standalone output"
    );
    verify_bundle(&app, "Perry & Bundle", "2.3.4");

    // Explicit .app outputs link inside the bundle. Rebuilding must not strip
    // after signing or truncate the executable while packaging it in place.
    for _ in 0..2 {
        let app = compile(root, &runtime, "main.ts", "nested/Explicit App.app");
        assert_eq!(app, root.join("nested/Explicit App.app"));
        assert!(!root.join("nested/Explicit App").exists());
        verify_bundle(&app, "Perry & Bundle", "2.3.4");
    }
    std::fs::write(
        root.join("perry.toml"),
        "[project]\nname = 'Project Name'\nbuild_number = 7\n[macos]\nbundle_id = 'dev.perry.bundle10078'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"type":"module","version":"5.6.7"}"#,
    )
    .unwrap();
    let app = compile(root, &runtime, "main.ts", "Metadata Fallback");
    verify_bundle(&app, "Project Name", "5.6.7");

    std::fs::write(root.join("cli.ts"), "console.log('plain cli');").unwrap();
    let cli = compile(root, &runtime, "cli.ts", "plain-cli");
    assert!(cli.is_file());
    assert!(!root.join("plain-cli.app").exists());
    let output = checked(Command::new(cli));
    assert_eq!(output.stdout, b"plain cli\n");
}
