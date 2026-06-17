//! Regression test for #5230: a non-resolvable (runtime-computed) dynamic
//! `import(...)` no longer blocks the build by default — it applies the same
//! defer/notice/strict policy as #5206's eval deferral.
//!
//! Default (non-strict) behavior:
//!   1. compilation SUCCEEDS even with a runtime-computed `import(spec)` in a
//!      cold (never-taken) branch,
//!   2. a visible end-of-compile NOTICE lists the degraded site under the SAME
//!      header as deferred eval sites (count + kind `import(...)` + `file:line`),
//!   3. the binary runs fine when the import path is never reached, including a
//!      sibling RESOLVABLE literal `import("./real.js")` that still loads, and
//!   4. if the deferred import IS reached it throws a descriptive, catchable
//!      `Error` (not a crash/segfault, not a silent no-op).
//!
//! Strict mode (`--strict-dynamic-import`, `perry.dynamicImport = "error"`, or
//! the broad `perry.strict = true`) restores the historical hard compile-time
//! refusal ("not statically resolvable").

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
/// the #5206 test; the CI `cargo-test` job doesn't pre-build the staticlib).
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

/// A resolvable sibling module — the literal `import("./real.js")` must still
/// compile and load it (regression guard for the resolvable forms).
const REAL_MODULE: &str = r#"export const greeting = "hello from real module";
"#;

/// The cold-path fixture:
///   - `import("./real.js")` is a string literal → resolvable → still loads.
///   - `import(p + ".js")` is runtime-computed → non-resolvable → deferred.
/// The deferred site is only reached behind `--force-import`.
const COLD_FIXTURE: &str = r#"
async function loadReal() {
  const m = await import("./real.js");
  console.log("REAL:" + m.greeting);
}

async function loadPlugin(name: string) {
  const p = name;
  const m = await import(p + ".js");
  return m;
}

await loadReal();

if (process.argv.indexOf("--force-import") !== -1) {
  try {
    await loadPlugin("./real");
    console.log("NO_THROW");
  } catch (e: any) {
    console.log("CAUGHT:" + (e && e.message));
  }
}
"#;

fn write_fixture(root: &std::path::Path) {
    std::fs::write(root.join("real.js"), REAL_MODULE).expect("write real.js");
    std::fs::write(root.join("main.ts"), COLD_FIXTURE).expect("write main.ts");
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
fn default_defer_compiles_prints_notice_runs_and_throws_on_reach() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);

    let out = compile(root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "default compile must succeed; stderr:\n{stderr}"
    );

    // The shared end-of-compile notice (same header as deferred eval) names the
    // dynamic-import kind and a `file:line`.
    assert!(
        stderr.contains("ahead-of-time-unsupported site"),
        "expected the shared deferred-site notice; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("import(...)"),
        "notice must name the dynamic-import kind; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("main.ts:9"),
        "notice must name the site location (file:line); stderr:\n{stderr}"
    );

    // The resolvable literal import loads; the deferred site is never reached.
    let bin = root.join("main_bin");
    let run = Command::new(&bin).output().expect("run compiled binary");
    assert!(run.status.success(), "binary must run the resolvable path");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("REAL:hello from real module"),
        "resolvable literal import must still load; got:\n{stdout}"
    );

    // Forcing the deferred path throws a descriptive, catchable Error.
    let run2 = Command::new(&bin)
        .arg("--force-import")
        .output()
        .expect("run compiled binary --force-import");
    let stdout2 = String::from_utf8_lossy(&run2.stdout);
    assert!(
        run2.status.success(),
        "the binary must not crash when the deferred import is reached"
    );
    assert!(
        stdout2.contains("CAUGHT:"),
        "the reached deferred import must throw a catchable Error; got:\n{stdout2}"
    );
    assert!(
        stdout2.contains("runtime-computed path cannot run in an ahead-of-time compiled binary"),
        "the thrown Error must be descriptive; got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("NO_THROW"),
        "the reached deferred import must NOT silently no-op; got:\n{stdout2}"
    );
}

#[test]
fn strict_dynamic_import_flag_refuses_at_compile_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);

    let out = compile(root, &["--strict-dynamic-import"]);
    assert!(
        !out.status.success(),
        "--strict-dynamic-import must fail the build for a runtime-computed import"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not statically resolvable"),
        "strict mode must print the not-resolvable refusal; stderr:\n{stderr}"
    );
}

#[test]
fn perry_strict_config_covers_dynamic_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);
    // The broad `perry.strict = true` must cover dynamic imports too.
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "strict-dynimport-cfg", "perry": { "strict": true } }"#,
    )
    .expect("write package.json");

    let out = compile(root, &[]);
    assert!(
        !out.status.success(),
        "perry.strict = true must fail the build for a runtime-computed import"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not statically resolvable"),
        "perry.strict must restore the refusal; stderr:\n{stderr}"
    );
}

#[test]
fn perry_dynamic_import_error_config_refuses() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "dynimport-error-cfg", "perry": { "dynamicImport": "error" } }"#,
    )
    .expect("write package.json");

    let out = compile(root, &[]);
    assert!(
        !out.status.success(),
        "perry.dynamicImport = \"error\" must fail the build"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not statically resolvable"),
        "dynamicImport=error must restore the refusal; stderr:\n{stderr}"
    );
}

#[test]
fn perry_dynamic_import_defer_overrides_broad_strict() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);
    // Dedicated knob (defer) wins over the broad strict for dynamic imports.
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "dynimport-override", "perry": { "strict": true, "dynamicImport": "defer" } }"#,
    )
    .expect("write package.json");

    let out = compile(root, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "perry.dynamicImport=\"defer\" must override perry.strict for imports; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("ahead-of-time-unsupported site"),
        "override must still defer + notice; stderr:\n{stderr}"
    );
}
