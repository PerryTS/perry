//! Regression test for #9486: `Error.prototype.stack` carries real, NAMED
//! frames instead of the single `at <anonymous>` placeholder.
//!
//! What is asserted is STRUCTURE, not bytes. Perry compiles to native code and
//! has no per-instruction line table, so a frame renders as `    at <name>`
//! where node renders `    at <name> (file:line:col)`; positions differ
//! legitimately and comparing them against node would be a test of the wrong
//! thing. What must hold is what the issue asks for: more than one frame, and
//! the function names in call order.
//!
//! Both halves of the mechanism are exercised: the frame-pointer capture at
//! construction and the address→name resolution on read. The fixture also
//! pins the re-throw contract — `throw e` inside a `catch` must NOT recapture,
//! so the innermost frame stays the one that first constructed the error.

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

/// `perry-runtime` is an rlib; the archive the compiled fixture links against
/// comes from the `perry-runtime-static` staticlib wrapper, so that is the
/// package to build. Building the rlib instead leaves `PERRY_RUNTIME_DIR`
/// pointing at a directory with no `libperry_runtime.a` (clean tree: the
/// fixture fails to link) or a stale one (reused target dir: the fixture
/// links a runtime older than the compiler that emitted its object code, and
/// perry's own source-hash check rejects the pair).
fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .output()
            .expect("run cargo build -p perry-runtime-static");
        assert!(
            build.status.success(),
            "cargo build -p perry-runtime-static failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("PERRY_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir);
    }
    ensure_runtime_archive();
    target_debug_dir()
}

/// Prints one `<label> <frame name>` line per frame so the assertions read
/// against a projection rather than against a whole stack string.
const FIXTURE: &str = r#"
function frameNames(stack: string): string[] {
  const out: string[] = [];
  for (const line of String(stack).split("\n")) {
    const t = line.trim();
    if (t.indexOf("at ") !== 0) continue;
    let name = t.slice(3);
    const paren = name.indexOf(" (");
    if (paren >= 0) name = name.slice(0, paren);
    out.push(name);
  }
  return out;
}

function report(label: string, stack: string): void {
  const names = frameNames(stack);
  console.log(label + " frames=" + names.length);
  for (const n of names) console.log(label + " name=" + n);
}

function chainC(): Error { return new Error("chain"); }
function chainB(): Error { return chainC(); }
function chainA(): Error { return chainB(); }

function innerThrow(): void { throw new Error("rethrown"); }
function outerRethrow(): void {
  try { innerThrow(); } catch (e) { throw e; }
}
function catchRethrow(): Error {
  try { outerRethrow(); } catch (e) { return e as Error; }
  return new Error("unreachable");
}

report("CHAIN", chainA().stack as string);
report("RETHROW", catchRethrow().stack as string);
"#;

fn compile_and_run(extra_args: &[&str]) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("main.ts"), FIXTURE).expect("write entry");
    let output = root.join("main_bin");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(root)
        .arg("compile")
        .arg(root.join("main.ts"))
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.env("PERRY_NO_AUTO_OPTIMIZE", "1");
    cmd.env("PERRY_RUNTIME_DIR", runtime_dir());
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile must succeed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&output).output().expect("run compiled binary");
    String::from_utf8_lossy(&run.stdout).into_owned()
}

fn names(stdout: &str, label: &str) -> Vec<String> {
    let needle = format!("{label} name=");
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix(&needle))
        .map(|s| s.to_string())
        .collect()
}

/// The headline of #9486: a captured stack names the JS functions it ran
/// through, in call order, instead of the single `at <anonymous>` placeholder.
///
/// The re-throw chain is what this asserts on, and deliberately so: its
/// `try`/`catch` frames survive LLVM's inliner, whereas the plain
/// `chainA → chainB → chainC` next door is small enough that `-O` folds all
/// three into their caller and there are no frames left to report. That
/// collapse is inherent to compiling ahead of time — V8 keeps inlined frames
/// because it retains inlining metadata for deoptimization, and perry has no
/// such record — so a test that demanded three frames there would be pinning
/// the optimizer's current appetite, not this fix.
#[test]
fn a_call_chain_reports_named_frames_in_call_order() {
    let stdout = compile_and_run(&[]);
    let frames = names(&stdout, "RETHROW");
    assert!(
        frames.len() >= 3,
        "expected at least three frames, not the single `<anonymous>` \
         placeholder #9486 is about; got {frames:?}\n{stdout}"
    );
    let joined = frames.join(",");
    for expected in ["innerThrow", "outerRethrow", "catchRethrow"] {
        assert!(
            frames.iter().any(|f| f.contains(expected)),
            "frame `{expected}` is missing from {joined}\n{stdout}"
        );
    }
    let pos = |name: &str| frames.iter().position(|f| f.contains(name)).unwrap();
    assert!(
        pos("innerThrow") < pos("outerRethrow") && pos("outerRethrow") < pos("catchRethrow"),
        "frames must run innermost-first like node's; got {joined}"
    );
    assert!(
        !frames.iter().any(|f| f == "<anonymous>"),
        "no frame should fall back to the placeholder here; got {joined}"
    );
}

/// `throw e` inside a `catch` re-throws the SAME error object. Node keeps the
/// original capture, so the innermost frame stays `innerThrow` — a stack whose
/// innermost frame were `catchRethrow` would mean the capture was redone at
/// the re-throw.
#[test]
fn a_rethrown_error_keeps_its_original_capture() {
    let stdout = compile_and_run(&[]);
    let frames = names(&stdout, "RETHROW");
    let inner = frames
        .iter()
        .position(|f| f.contains("innerThrow"))
        .unwrap_or_else(|| panic!("no `innerThrow` frame in {frames:?}\n{stdout}"));
    assert_eq!(
        inner, 0,
        "the capture must be the one taken where the error was CONSTRUCTED \
         (`innerThrow` is the innermost JS frame), not where it was \
         re-thrown; got {frames:?}\n{stdout}"
    );
}

/// Even where inlining leaves nothing of the call chain, the stack must not
/// fall back to the placeholder: the frame that remains has to be named.
#[test]
fn an_inlined_chain_still_names_the_frame_that_survives() {
    let stdout = compile_and_run(&[]);
    let frames = names(&stdout, "CHAIN");
    assert!(
        !frames.is_empty(),
        "a stack with no frames at all; got {stdout}"
    );
    assert!(
        frames.iter().all(|f| f != "<anonymous>"),
        "every surviving frame must be named — `<anonymous>` is the \
         pre-#9486 placeholder; got {frames:?}\n{stdout}"
    );
}
