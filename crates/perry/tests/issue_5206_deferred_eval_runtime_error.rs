//! Regression test for #5206: a runtime-unknown `eval(...)` / `new
//! Function(<dynamic body>)` site no longer blocks the build by default.
//!
//! The default (non-strict) behavior:
//!   1. compilation SUCCEEDS even with such a site in a cold (never-taken)
//!      branch,
//!   2. a visible end-of-compile NOTICE lists the degraded site(s)
//!      (count + kind + `file:line`),
//!   3. the binary runs fine when the eval path is never reached, and
//!   4. if the eval path IS reached at runtime it is handled by the #6559
//!      dyn-eval interpreter: INVALID source (this fixture's `return (not
//!      json)`) throws a catchable `SyntaxError` — matching Node — rather
//!      than crashing or silently no-opping; valid subset-supported source
//!      now actually runs.
//!
//! Strict-eval mode (CLI `--strict-eval` or `perry.eval = "error"` /
//! `perry.strict = true` config) restores the historical hard compile-time
//! refusal.

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

/// Build `libperry_runtime.a` once so the compiled binaries can link. The CI
/// `cargo-test` job doesn't pre-build the runtime staticlib, and these tests
/// link real executables, so they'd otherwise fail with "Could not find
/// libperry_runtime.a" (mirrors module_import_forms.rs).
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

/// The cold-path fixture: `new Function` is only reached in the JSON-parse
/// `catch`. The default `console.log` exercises the JSON path, so the deferred
/// site is never invoked unless the caller forces it.
const COLD_FIXTURE: &str = r#"
function parseParams(s: string): any {
  try { return JSON.parse(s); } catch { return new Function(`return (${s})`)(); }
}
// JSON path — never hits new Function.
console.log(parseParams('{"a":1}').a);

// Force the deferred path only when asked, and prove it throws a catchable
// Error rather than crashing.
if (process.argv.indexOf("--force-eval") !== -1) {
  try {
    parseParams("not json");
    console.log("NO_THROW");
  } catch (e: any) {
    console.log("CAUGHT:" + (e && e.message));
  }
}
"#;

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
    // Pure-language program — skip auto-optimize to avoid runtime rebuilds.
    cmd.env("PERRY_NO_AUTO_OPTIMIZE", "1");
    cmd.env("PERRY_RUNTIME_DIR", runtime_dir());
    cmd.output().expect("run perry compile")
}

#[test]
fn default_defer_compiles_prints_notice_and_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), COLD_FIXTURE).expect("write entry");

    let out = compile(root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "default compile must succeed; stderr:\n{stderr}"
    );

    // The visible end-of-compile notice: count + kind + file:line. #5230
    // generalized the headline ("ahead-of-time-unsupported site(s)") so eval and
    // dynamic-import deferrals share one notice.
    assert!(
        stderr.contains("ahead-of-time-unsupported site"),
        "expected the deferred-eval notice; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("new Function(...)"),
        "notice must name the site kind; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("main.ts:3"),
        "notice must name the site location; stderr:\n{stderr}"
    );

    // The JSON path runs fine (the deferred site is never invoked).
    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    assert!(run.status.success(), "binary must run on the JSON path");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(stdout.trim(), "1", "JSON path must print 1; got:\n{stdout}");

    // Forcing the deferred path: `new Function("return (not json)")` is
    // invalid source, so the #6559 interpreter throws a catchable
    // SyntaxError at construction (Node behavior) — the descriptive,
    // catchable signal this test has always demanded.
    let run2 = Command::new(&bin)
        .arg("--force-eval")
        .output()
        .expect("run compiled binary --force-eval");
    let stdout2 = String::from_utf8_lossy(&run2.stdout);
    assert!(
        stdout2.contains("CAUGHT:"),
        "the invoked deferred site must throw a catchable error; got:\n{stdout2}"
    );
    assert!(
        stdout2.contains("invalid or unsupported source"),
        "the thrown error must be descriptive; got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("NO_THROW"),
        "the invoked deferred site must NOT silently no-op; got:\n{stdout2}"
    );
}

#[test]
fn strict_eval_flag_refuses_at_compile_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), COLD_FIXTURE).expect("write entry");

    let out = compile(root, &["--strict-eval"]);
    assert!(
        !out.status.success(),
        "strict-eval mode must fail the build for a runtime-unknown site"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is refused at compile time"),
        "strict-eval must print the refusal message; stderr:\n{stderr}"
    );
}

#[test]
fn perry_eval_error_config_refuses_at_compile_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), COLD_FIXTURE).expect("write entry");
    // Config-driven strict mode via package.json `perry.eval = "error"`.
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "strict-eval-cfg", "perry": { "eval": "error" } }"#,
    )
    .expect("write package.json");

    let out = compile(root, &[]);
    assert!(
        !out.status.success(),
        "perry.eval = \"error\" must fail the build for a runtime-unknown site"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is refused at compile time"),
        "config strict mode must print the refusal message; stderr:\n{stderr}"
    );
}

#[test]
fn allow_eval_env_overrides_strict_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), COLD_FIXTURE).expect("write entry");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "allow-eval-override", "perry": { "eval": "error" } }"#,
    )
    .expect("write package.json");

    // PERRY_ALLOW_EVAL=1 forces non-strict even though config asked for error.
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_ALLOW_EVAL", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "PERRY_ALLOW_EVAL=1 must override a strict config and build; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("ahead-of-time-unsupported site"),
        "back-compat override must still print the deferred notice; stderr:\n{stderr}"
    );
}
