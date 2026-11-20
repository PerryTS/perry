//! Every shared-band handle id an http server retires is retired on the agent
//! whose heap can name it, so the full trace of that heap decides its reuse.
//! An id retired on a thread that runs no collector has no heap to decide it:
//! it is only counted and adopted by the next tracing mutator. This test makes
//! that case fatal (`PERRY_HANDLE_OFF_MUTATOR_FATAL`, read only on that path)
//! and runs the keep-alive identity program and the 300k-request id-pressure
//! program, so an off-mutator retirement on either path cannot grow silently.
//! Expected output is node's.

use std::path::Path;
use std::process::Command;

fn compile_and_run(source: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let entry = dir.path().join("main.ts");
    let binary = dir
        .path()
        .join(if cfg!(windows) { "main.exe" } else { "main" });
    std::fs::write(&entry, source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(dir.path())
        .args(["compile", "--no-cache"])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    run(&binary, dir.path(), expected);
}

fn run(binary: &Path, dir: &Path, expected: &str) {
    let output = Command::new(binary)
        .current_dir(dir)
        .env("PERRY_HANDLE_OFF_MUTATOR_FATAL", "1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && !stderr.contains("retired off a mutator thread"),
        "run failed: {:?}\n{}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        expected
    );
}

#[test]
fn keepalive_requests_retire_their_ids_on_the_mutator() {
    compile_and_run(
        include_str!(
            "../../../tests/parity/node-suite/http/test_gap_http_keepalive_request_identity.ts"
        ),
        "phase 1 held 50 wrong 0\nphase 2 held 50 wrong 0\nrequests 100 bodies 100\nerrors none\n",
    );
}

#[test]
fn id_pressure_retires_every_id_on_the_mutator() {
    compile_and_run(
        include_str!("../../../tests/parity/node-suite/http/test_gap_http_handle_id_pressure.ts"),
        "served 300000 answered 300000 failed 0\nkept 300 intact 300 same as kept 0 weak hits 0\n",
    );
}
