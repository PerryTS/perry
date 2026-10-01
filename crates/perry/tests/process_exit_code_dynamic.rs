//! Dynamic process.exitCode uses the same status as the intrinsic (#11622).
use std::path::PathBuf;
use std::process::Command;

#[test]
fn dynamic_process_exit_code_matches_node() {
    let dir = tempfile::tempdir().unwrap();
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let cases = [
        ("global", "globalThis.process.exitCode = 3; globalThis.process.exit();", 3),
        ("alias", "const p = globalThis.process; p.exitCode = 4; p.exit();", 4),
        ("import", "import p from 'node:process'; p.exitCode = 5; p.exit();", 5),
        ("natural", "const p = globalThis.process; p.exitCode = 6;", 6),
        ("explicit", "const p = globalThis.process; p.exitCode = 7; p.exit(0);", 0),
        ("deferred", "const p = globalThis.process; p.exitCode = 3; p.stdout.write('', () => p.stderr.write('', () => setImmediate(() => p.exit())));", 3),
        ("coherent", "const p = globalThis.process; p.exitCode = '3'; console.log(p.exitCode, process.exitCode); process.exitCode = 4; console.log(p.exitCode); p.exitCode = undefined; console.log(p.exitCode, process.exitCode);", 0),
    ];
    for (name, source, expected_status) in cases {
        let entry = dir.path().join(format!("{name}.ts"));
        std::fs::write(&entry, source).unwrap();
        let oracle = Command::new("node").arg(&entry).output().unwrap();
        assert_eq!(oracle.status.code(), Some(expected_status), "Node {name}");
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let executable = dir.path().join(format!("{name}-native{suffix}"));
        let compile = Command::new(&compiler)
            .env("PERRY_RUNTIME_DIR", &runtime)
            .env("PERRY_WORKSPACE_ROOT", &workspace)
            .args(["compile", "--no-cache", "--no-auto-optimize"])
            .arg(&entry)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compile.status.success(),
            "{name} compile: {}",
            String::from_utf8_lossy(&compile.stderr)
        );
        let native = Command::new(executable).output().unwrap();
        assert_eq!(
            native.status.code(),
            oracle.status.code(),
            "{name} stderr: {}",
            String::from_utf8_lossy(&native.stderr)
        );
        assert_eq!(native.stdout, oracle.stdout, "{name}");
    }
}
