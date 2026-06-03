//! Phase D: End-to-end test harness for `perry/container`.
//!
//! Each test under `tests/` invokes the perry CLI to compile a real
//! `.e2e.ts` file under `<workspace>/tests/e2e/`, runs the resulting
//! binary, and asserts the stdout contract (`[e2e] PASS` on success,
//! `[e2e] FAIL: ...` on failure). The full TS → HIR → codegen → FFI
//! → engine → backend → docker chain is exercised.
//!
//! All e2e tests are env-gated: without `PERRY_E2E_TESTS=1` they
//! skip with a log line. Run locally:
//!
//! ```text
//! PERRY_E2E_TESTS=1 \
//! PERRY_CONTAINER_BACKEND=docker \
//! cargo test -p perry-container-e2e -- --test-threads=1
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Where the workspace root lives (parent of this crate's manifest dir).
pub fn workspace_root() -> PathBuf {
    let manifest_dir: PathBuf = env!("CARGO_MANIFEST_DIR").into();
    manifest_dir
        .parent() // crates/
        .and_then(|p| p.parent()) // workspace root
        .unwrap_or(&manifest_dir)
        .to_path_buf()
}

/// Are e2e tests opted in via env?
pub fn e2e_enabled() -> bool {
    std::env::var("PERRY_E2E_TESTS").as_deref() == Ok("1")
}

/// Locate the released `perry` binary. Prefers the workspace's
/// `target/release/perry` (built by CI before invoking these tests);
/// falls back to `target/debug/perry` for local dev.
pub fn perry_binary() -> PathBuf {
    let root = workspace_root();
    for profile in ["release", "debug"] {
        let p = root.join("target").join(profile).join("perry");
        if p.exists() {
            return p;
        }
    }
    panic!(
        "could not find `perry` binary under {}/target/{{release,debug}}; \
         build it first: `cargo build --release -p perry`",
        root.display()
    );
}

/// Result of running an e2e program.
pub struct E2eResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Compile + run a `.e2e.ts` file under `tests/e2e/`. Honors a
/// per-test `extra_env` to set `PERRY_E2E_PORT` etc.
pub fn run_e2e(name: &str, extra_env: &[(&str, &str)]) -> E2eResult {
    let root = workspace_root();
    let src = root.join("tests").join("e2e").join(format!("{}.e2e.ts", name));
    assert!(
        src.exists(),
        "missing e2e source: {}",
        src.display()
    );

    // Compile to a per-test binary in a tmp subdir so parallel runs
    // (if a future user disables --test-threads=1) don't clobber.
    let out_dir = root.join("target").join("e2e-bin");
    std::fs::create_dir_all(&out_dir).expect("mkdir target/e2e-bin");
    let bin_path = out_dir.join(name);

    let perry = perry_binary();
    let compile_out = Command::new(&perry)
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(&bin_path)
        .output()
        .expect("invoke perry compile");
    if !compile_out.status.success() {
        panic!(
            "perry compile failed for {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            name,
            String::from_utf8_lossy(&compile_out.stdout),
            String::from_utf8_lossy(&compile_out.stderr)
        );
    }

    // Run with a 5-minute walltime ceiling (image pulls can be slow).
    let run = run_with_timeout(&bin_path, extra_env, Duration::from_secs(300))
        .expect("run e2e binary");

    E2eResult {
        exit_code: run.0,
        stdout: run.1,
        stderr: run.2,
    }
}

fn run_with_timeout(
    bin: &Path,
    env: &[(&str, &str)],
    timeout: Duration,
) -> Option<(i32, String, String)> {
    use std::sync::mpsc::{channel, RecvTimeoutError};
    let bin_owned = bin.to_path_buf();
    let bin_for_panic = bin.to_path_buf();
    let env_owned: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut cmd = Command::new(&bin_owned);
        for (k, v) in &env_owned {
            cmd.env(k, v);
        }
        let out = cmd.output();
        let _ = tx.send(out);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => Some((
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )),
        Ok(Err(_)) => None,
        Err(RecvTimeoutError::Timeout) => panic!(
            "e2e program {} exceeded {:?} walltime",
            bin_for_panic.display(),
            timeout
        ),
        Err(_) => None,
    }
}

/// Assert the standard `[e2e] PASS` contract on stdout + exit 0.
pub fn assert_e2e_pass(name: &str, result: &E2eResult) {
    assert_eq!(
        result.exit_code, 0,
        "e2e test `{}` exited with {} \n--- stdout ---\n{}\n--- stderr ---\n{}",
        name, result.exit_code, result.stdout, result.stderr
    );
    assert!(
        result.stdout.contains("[e2e] PASS"),
        "e2e test `{}` did not print `[e2e] PASS`\n--- stdout ---\n{}\n--- stderr ---\n{}",
        name,
        result.stdout,
        result.stderr
    );
}
