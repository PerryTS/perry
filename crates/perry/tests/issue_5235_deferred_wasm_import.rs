//! Regression test for #5235: an `import ... from "./x.wasm"` no longer
//! hard-fails the build (the file is binary, not UTF-8). It applies the same
//! defer / notice / strict policy as #5206 (eval) and #5230 (dynamic import).
//!
//! Default (non-strict) behavior:
//!   1. compilation SUCCEEDS — the `.wasm` is read as bytes, its export section
//!      parsed, and a throwing-stub module synthesized,
//!   2. a visible end-of-compile NOTICE lists the degraded site under the SAME
//!      header as deferred eval / dynamic-import sites (kind `.wasm import`),
//!   3. importing the module but never calling its exports runs fine, and
//!   4. calling an export (`w.add(2,3)`) throws a descriptive, catchable
//!      `Error` referencing #5234 — not a crash/segfault, not a silent no-op.
//!
//! Strict mode (`--strict-dynamic-import` or the broad `perry.strict = true`)
//! turns the `.wasm` import into a hard compile error.

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
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

/// Build `libperry_runtime.a` once so the compiled binaries can link (mirrors
/// the #5206 / #5230 tests; CI's `cargo-test` job doesn't pre-build the staticlib).
fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime")
            .output()
            .expect("run cargo build -p perry-runtime");
        assert!(
            build.status.success(),
            "cargo build -p perry-runtime failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

/// The 41-byte `add.wasm` fixture from #5235 — exports a single `add` function.
const ADD_WASM_BASE64: &str = "AGFzbQEAAAABBwFgAn9/AX8DAgEABwcBA2FkZAAACgkBBwAgACABags=";

/// Fixture program: imports the wasm namespace, calls `add` (which must throw),
/// catches it, and prints — proving the deferred export throws only on call.
const MAIN_FIXTURE: &str = r#"
import * as w from "./add.wasm";

console.log("IMPORTED");

if (process.argv.indexOf("--call") !== -1) {
  try {
    const r = (w as any).add(2, 3);
    console.log("NO_THROW:" + r);
  } catch (e: any) {
    console.log("CAUGHT:" + (e && e.message));
  }
}
console.log("DONE");
"#;

fn write_fixture(root: &std::path::Path) {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(ADD_WASM_BASE64)
        .expect("decode add.wasm fixture");
    assert_eq!(bytes.len(), 41, "add.wasm fixture must be 41 bytes");
    std::fs::write(root.join("add.wasm"), &bytes).expect("write add.wasm");
    std::fs::write(root.join("main.ts"), MAIN_FIXTURE).expect("write main.ts");
}

fn compile(root: &std::path::Path, extra_args: &[&str]) -> std::process::Output {
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.env("PERRY_NO_AUTO_OPTIMIZE", "1");
    cmd.env("PERRY_RUNTIME_DIR", runtime_dir());
    cmd.output().expect("run perry compile")
}

#[test]
fn default_defer_compiles_prints_notice_runs_and_throws_on_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);

    let out = compile(root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "default compile must succeed for a .wasm import; stderr:\n{stderr}"
    );

    // Shared end-of-compile notice (same header as deferred eval/dyn-import).
    assert!(
        stderr.contains("ahead-of-time-unsupported site"),
        "expected the shared deferred-site notice; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains(".wasm import"),
        "notice must name the .wasm import kind; stderr:\n{stderr}"
    );

    // Importing but never calling runs fine.
    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "binary must run when no wasm export is called"
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("IMPORTED") && stdout.contains("DONE") && !stdout.contains("CAUGHT:"),
        "import-only program must run to completion without throwing; got:\n{stdout}"
    );

    // Calling the export throws a descriptive, catchable Error referencing #5234.
    let run2 = Command::new(&bin)
        .arg("--call")
        .output()
        .expect("run compiled binary --call");
    assert!(
        run2.status.success(),
        "the binary must not crash when a wasm export is called"
    );
    let stdout2 = String::from_utf8_lossy(&run2.stdout);
    assert!(
        stdout2.contains("CAUGHT:"),
        "the called wasm export must throw a catchable Error; got:\n{stdout2}"
    );
    assert!(
        stdout2.contains("cannot run in an ahead-of-time compiled binary")
            && stdout2.contains("#5234"),
        "the thrown Error must be descriptive and reference #5234; got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("NO_THROW"),
        "the called wasm export must NOT silently return a value; got:\n{stdout2}"
    );
}

#[test]
fn strict_dynamic_import_flag_refuses_wasm_at_compile_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);

    let out = compile(root, &["--strict-dynamic-import"]);
    assert!(
        !out.status.success(),
        "--strict-dynamic-import must fail the build for a .wasm import"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(".wasm import") && stderr.contains("strict mode"),
        "strict mode must print the .wasm refusal; stderr:\n{stderr}"
    );
}

#[test]
fn perry_strict_config_covers_wasm_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "strict-wasm-cfg", "perry": { "strict": true } }"#,
    )
    .expect("write package.json");

    let out = compile(root, &[]);
    assert!(
        !out.status.success(),
        "perry.strict = true must fail the build for a .wasm import"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(".wasm import"),
        "perry.strict must restore the .wasm refusal; stderr:\n{stderr}"
    );
}
