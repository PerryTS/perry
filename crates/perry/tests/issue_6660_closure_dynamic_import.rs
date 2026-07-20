//! Regression tests for #6660 (pi wall #8): a dynamic `import()` inside a
//! CLOSURE — `const imp = (s) => import(s)` — rejected with literal
//! `undefined`, killing the one-shot flow with a reasonless
//! `Uncaught (in promise) undefined` where node completes normally.
//!
//! Trigger in the wild: pi-ai's auth context helper
//! (`importNodeModule = (specifier) => import(rewrite(specifier))`,
//! awaited inside `async fileExists` with a swallowing try/catch) — the
//! rejected promise's `undefined` reason escaped as an unhandled rejection
//! before the first stdout write of the `-p` flow.
//!
//! Two root causes, both covered here:
//!
//! 1. Visitor asymmetry (perry-hir `dynamic_import/visitors.rs`): the
//!    read-only `for_each_dynamic_import` did NOT descend into
//!    `Expr::Closure` bodies while its `_mut` sibling did. The driver aligns
//!    resolution outcomes 1:1 by traversal order (collect via ref visitor,
//!    fill via mut visitor), so every closure-nested `import()` was invisible
//!    to the resolver but still consumed a slot in the fill pass — the site
//!    kept empty `paths` with no `deferred_error` and codegen lowered it to
//!    its defensive `js_promise_rejected(undefined)` arm.
//!
//! 2. Reasonless fallthroughs (perry-codegen `dyn_extern_i18n.rs`): the
//!    empty-paths / unmapped-target / no-match arms rejected with literal
//!    `undefined`. They now route through the runtime fallback
//!    (`js_module_dynamic_import_fallback` — the `import()` analog of the
//!    #5389 ambient-require fallthrough): node builtins resolve by string to
//!    the same namespace `require(spec)` produces, anything else rejects
//!    with a descriptive `Error` carrying `code: 'ERR_MODULE_NOT_FOUND'`.
//!    The #5230 deferred arm keeps its site-specific `file:line` message for
//!    unknown modules but now also resolves builtins at runtime.
//!
//! Expected outputs are node v26's, byte for byte (stdout + stderr + rc).

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
/// the #5230 test; the CI `cargo-test` job doesn't pre-build the staticlib).
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

/// Compile `main.ts` in `dir` and run it, returning (stdout, stderr, rc).
fn compile_and_run(dir: &std::path::Path, source: &str) -> (String, String, i32) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    (
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
        run.status.code().unwrap_or(-1),
    )
}

/// The wild shape, minimized: pi-ai's `importNodeModule` closure (specifier
/// routed through a rewrite helper, so the resolver cannot const-fold it) and
/// its `async fileExists` consumer with the swallowing try/catch. Node prints
/// `true` / `false`; the pre-fix binary printed `false` / `false` (dynamic
/// import of `node:fs/promises` rejected) and — in pi's larger flow — died
/// with `Uncaught (in promise) undefined`.
#[test]
fn closure_dynamic_import_of_builtin_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, stderr, rc) = compile_and_run(
        dir.path(),
        r#"
const rewrite = (p: string) => {
  if (typeof p === "string" && /^\.\.?\//.test(p)) {
    return p.replace(/\.ts$/i, ".js");
  }
  return p;
};
const importNodeModule = (specifier: string) => import(rewrite(specifier));

async function fileExists(path: string): Promise<boolean> {
  try {
    const fs = await importNodeModule("node:fs/promises");
    await fs.access(path);
    return true;
  } catch {
    return false;
  }
}

console.log(await fileExists("/"));
console.log(await fileExists("/definitely-missing-perry-6660"));
"#,
    );
    assert_eq!(stdout, "true\nfalse\n");
    assert_eq!(stderr, "");
    assert_eq!(rc, 0);
}

/// Bare param-passthrough closure (`const imp = (s) => import(s)`) — the
/// minimal visitor-asymmetry trigger — resolving both a submodule-spec
/// builtin (`node:fs/promises`) and a native-module builtin (`node:os`).
/// Under the broken visitor this printed nothing and died with
/// `Uncaught exception: undefined` at the top-level await.
#[test]
fn closure_dynamic_import_passthrough_builtins_match_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, stderr, rc) = compile_and_run(
        dir.path(),
        r#"
const imp = (s: string) => import(s);
const fsp = await imp("node:fs/promises");
console.log("fs/promises access:", typeof fsp.access);
const os = await imp("node:os");
console.log("os homedir:", typeof os.homedir);
console.log("homedir is string:", typeof os.homedir() === "string");
"#,
    );
    assert_eq!(
        stdout,
        "fs/promises access: function\nos homedir: function\nhomedir is string: true\n"
    );
    assert_eq!(stderr, "");
    assert_eq!(rc, 0);
}

/// A runtime-computed specifier that names an UNKNOWN module must reject with
/// a real, catchable `Error` (code `ERR_MODULE_NOT_FOUND` family) — never
/// with literal `undefined`. Message text differs from node (node names the
/// importing file; an AOT binary reports the deferral site), so this pins
/// the behavior contract rather than bytes: caught, `instanceof Error`,
/// non-empty message, and reason !== undefined.
#[test]
fn closure_dynamic_import_unknown_module_rejects_with_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (stdout, stderr, rc) = compile_and_run(
        dir.path(),
        r#"
const imp = (s: string) => import(s);
try {
  await imp("definitely-not-a-module-perry-6660");
  console.log("NO_THROW");
} catch (e: any) {
  console.log("caught error:", e instanceof Error);
  console.log("reason defined:", e !== undefined);
  console.log("has message:", typeof e.message === "string" && e.message.length > 0);
}
"#,
    );
    assert_eq!(
        stdout,
        "caught error: true\nreason defined: true\nhas message: true\n"
    );
    assert_eq!(stderr, "");
    assert_eq!(rc, 0);
}

/// Outcome-alignment guard: a module with BOTH a closure-nested dynamic
/// import and a top-level literal one. Before the visitor fix the collection
/// pass saw only the top-level site while the fill pass visited both in
/// order, so outcomes could cross-assign between sites. Both must work, in
/// both call orders.
#[test]
fn closure_and_toplevel_dynamic_imports_stay_aligned() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("real.ts"),
        "export const greeting = \"hello from real module\";\n",
    )
    .expect("write real.ts");
    let (stdout, stderr, rc) = compile_and_run(
        dir.path(),
        r#"
const imp = (s: string) => import(s);
const os = await imp("node:os");
const real = await import("./real.ts");
console.log("closure-first os:", typeof os.homedir);
console.log("literal real:", real.greeting);
const os2 = await imp("node:os");
console.log("closure-again os:", typeof os2.homedir);
"#,
    );
    assert_eq!(
        stdout,
        "closure-first os: function\nliteral real: hello from real module\nclosure-again os: function\n"
    );
    assert_eq!(stderr, "");
    assert_eq!(rc, 0);
}
