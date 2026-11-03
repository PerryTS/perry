//! Local acceptance against the exact native-preview executable, without CI
//! downloads. Run with TSGO_TEST_BINARY=/absolute/path/to/tsgo cargo test -p
//! perry --test tsgo_api_client -- --ignored --nocapture.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry(project: &Path, checked: bool, output: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_perry"));
    cmd.current_dir(project)
        .args(["compile", "main.ts", "--no-auto-optimize", "-o"])
        .arg(output);
    if checked {
        cmd.arg("--type-check");
    }
    cmd.output().unwrap()
}
fn log(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn check_project(files: usize) {
    let binary =
        PathBuf::from(std::env::var_os("TSGO_TEST_BINARY").expect("TSGO_TEST_BINARY required"));
    assert!(binary.is_absolute());
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    std::fs::create_dir_all(project.join("node_modules/.bin")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&binary, project.join("node_modules/.bin/tsgo")).unwrap();
    #[cfg(not(unix))]
    std::fs::copy(&binary, project.join("node_modules/.bin/tsgo.exe")).unwrap();
    std::fs::write(project.join("tsconfig.json"), r#"{"compilerOptions":{"strict":true,"target":"ES2022","module":"NodeNext","moduleResolution":"NodeNext","allowImportingTsExtensions":true,"noEmit":true},"include":["*.ts"]}"#).unwrap();
    let mut main = String::new();
    for i in 0..files - 1 {
        std::fs::write(
            project.join(format!("f{i}.ts")),
            format!("export const v{i} = {i};\n"),
        )
        .unwrap();
        main.push_str(&format!("import {{v{i}}} from './f{i}.ts';\n"));
    }
    main.push_str(&format!(
        "console.log({});\n",
        (0..files - 1)
            .map(|i| format!("v{i}"))
            .collect::<Vec<_>>()
            .join("+")
    ));
    std::fs::write(project.join("main.ts"), main).unwrap();
    let oracle = || {
        Command::new(&binary)
            .current_dir(project)
            .args(["--project", "tsconfig.json", "--pretty", "false"])
            .output()
            .unwrap()
    };
    let good = oracle();
    assert!(good.status.success(), "{}", log(&good));
    let output = project.join("app");
    let compile = perry(project, true, &output);
    assert!(compile.status.success(), "{}", log(&compile));
    assert!(log(&compile).contains("Type checking enabled"));
    assert!(!log(&compile).contains("Warning:"), "{}", log(&compile));
    let native = Command::new(&output).output().unwrap();
    let node = Command::new("node")
        .current_dir(project)
        .arg("main.ts")
        .output()
        .unwrap();
    assert!(native.status.success());
    assert!(node.status.success(), "{}", log(&node));
    assert_eq!(native.stdout, node.stdout);
    // The last imported module, not merely the entry, must be checked.
    let bad_file = format!("f{}.ts", files - 2);
    std::fs::write(
        project.join(&bad_file),
        format!("export const v{}: string = 42;\n", files - 2),
    )
    .unwrap();
    let bad = oracle();
    assert!(!bad.status.success());
    let diagnostic_lines: Vec<_> = String::from_utf8_lossy(&bad.stdout)
        .lines()
        .filter(|line| line.contains("error TS"))
        .map(str::to_owned)
        .collect();
    assert_eq!(diagnostic_lines.len(), 1, "{}", log(&bad));
    let compile = perry(project, true, &project.join("bad-app"));
    assert!(!compile.status.success(), "{}", log(&compile));
    for line in diagnostic_lines {
        assert!(
            log(&compile).contains(&line),
            "oracle={line}\n{}",
            log(&compile)
        );
    }
    println!("{files} files: clean pass, node parity, deliberate TS2322 matches standalone");
}

#[test]
#[ignore = "requires pinned native-preview and compiled runtime archives"]
fn real_tsgo_ten_files() {
    check_project(10);
}
#[test]
#[ignore = "requires pinned native-preview and compiled runtime archives"]
fn real_tsgo_three_hundred_files() {
    check_project(300);
}

#[cfg(unix)]
#[test]
#[ignore = "requires compiled runtime archives"]
fn unsupported_tsgo_versions_continue_without_opening_ipc() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path();
    std::fs::write(project.join("main.ts"), "console.log(42);\n").unwrap();
    std::fs::write(project.join("tsconfig.json"), "{}").unwrap();
    let bin_dir = project.join("node_modules/.bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let checker = bin_dir.join("tsgo");
    for version in ["7.0.0-dev.20250101.1", "7.0.0-dev.20990101.1"] {
        std::fs::write(&checker, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'Version {version}'; else touch '{}'; exit 9; fi\n",project.join("opened-ipc").display())).unwrap();
        std::fs::set_permissions(&checker, std::fs::Permissions::from_mode(0o755)).unwrap();
        let output = perry(project, true, &project.join("app"));
        assert!(output.status.success(), "{}", log(&output));
        assert!(
            log(&output).contains("unsupported tsgo API version"),
            "{}",
            log(&output)
        );
        assert!(log(&output).contains("Continuing without type checking"));
        assert!(!project.join("opened-ipc").exists());
    }
    std::fs::remove_file(checker).unwrap();
    let output = perry(project, true, &project.join("app"));
    assert!(output.status.success(), "{}", log(&output));
    assert!(log(&output).contains("tsgo not found"), "{}", log(&output));
}
