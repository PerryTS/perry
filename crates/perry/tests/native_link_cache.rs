use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_json(project: &Path, entry: &Path, output: &Path) -> Value {
    let out = Command::new(perry_bin())
        .current_dir(project)
        .arg("--format")
        .arg("json")
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(output)
        .output()
        .expect("run perry compile");

    assert!(
        out.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .expect("json line in stdout");
    serde_json::from_str(line).expect("parse compile json")
}

fn run_binary(output: &Path) -> String {
    let out = Command::new(output).output().expect("run compiled binary");
    assert!(
        out.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn assert_linked(result: &Value) {
    assert_eq!(result["link_cache"]["linked"], true, "{result}");
    assert_eq!(result["link_cache"]["skipped"], false, "{result}");
}

fn assert_skipped(result: &Value) {
    assert_eq!(result["link_cache"]["linked"], false, "{result}");
    assert_eq!(result["link_cache"]["skipped"], true, "{result}");
}

#[test]
fn native_compile_skips_link_on_identical_second_build() {
    let dir = tempfile::tempdir().expect("tempdir");
    let project = dir.path();
    let src = project.join("src");
    let dist = project.join("dist");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dist).unwrap();
    fs::write(
        project.join("package.json"),
        "{\"name\":\"link-cache-test\"}\n",
    )
    .unwrap();
    fs::write(
        src.join("util.ts"),
        "export function answer(): number { return 41; }\n",
    )
    .unwrap();
    fs::write(
        src.join("main.ts"),
        "import { answer } from './util';\nconsole.log(answer() + 1);\n",
    )
    .unwrap();

    let entry = src.join("main.ts");
    let output = dist.join("app");

    let first = compile_json(project, &entry, &output);
    assert_linked(&first);
    let first_bytes = fs::read(&output).expect("first output");
    assert_eq!(run_binary(&output).trim(), "42");

    let second = compile_json(project, &entry, &output);
    assert_skipped(&second);
    assert_eq!(fs::read(&output).expect("second output"), first_bytes);
    assert_eq!(run_binary(&output).trim(), "42");

    fs::write(
        src.join("util.ts"),
        "export function answer(): number { return 40; }\n",
    )
    .unwrap();
    let changed = compile_json(project, &entry, &output);
    assert_linked(&changed);
    assert_eq!(run_binary(&output).trim(), "41");

    fs::remove_file(&output).unwrap();
    let missing_output = compile_json(project, &entry, &output);
    assert_linked(&missing_output);
    assert!(output.exists());
}
