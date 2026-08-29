//! Compile/run regressions for source-ordered lexical declarations in classic
//! `for` heads (#9052).

use std::path::PathBuf;
use std::process::Command;
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
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"));
    if cfg!(windows) {
        target.join("x86_64-pc-windows-msvc").join("debug")
    } else {
        target.join("debug")
    }
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static");
        if cfg!(windows) {
            command.arg("--target").arg("x86_64-pc-windows-msvc");
        }
        let build = command.output().expect("build runtime archives");
        assert!(
            build.status.success(),
            "runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("create tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write fixture");
    ensure_runtime_archive();

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("--no-cache")
        .arg("-o")
        .arg(&binary)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", target_debug_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&binary)
        .current_dir(dir.path())
        .output()
        .expect("run compiled fixture");
    assert!(
        run.status.success(),
        "compiled fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn later_initializer_reads_earlier_lexical_binding() {
    let stdout = compile_and_run(
        r#"
for (let i = 0, limit = 2, next = i + 1; i < limit; i++, next++) {
  console.log(i, next);
}
"#,
    );
    assert_eq!(stdout, "0 1\n1 2\n");
}

#[test]
fn destructuring_and_per_iteration_capture_keep_node_semantics() {
    let stdout = compile_and_run(
        r#"
const callbacks: Array<() => string> = [];
for (let i = 0, [next] = [i + 1]; i < 2; i++, next++) {
  callbacks.push(() => `${i}:${next}`);
}
console.log(callbacks.map((callback) => callback()).join(","));
"#,
    );
    assert_eq!(stdout, "0:1,1:2\n");
}
