//! `Function.apply(null, <args assembled at runtime>)` is a CreateDynamicFunction
//! surface, exactly like `new Function(body)` — the constructor is merely reached
//! indirectly. Perry cannot compile a body built from runtime data, so the site must
//! be classified as an eval surface and lowered to the deferred, located
//! "cannot run in an ahead-of-time compiled binary" error.
//!
//! Before the fix the classifier only recognized `Function.apply(this, [<literal
//! array>])`. A runtime-built argument list fell through to the generic lowering and
//! evaluated to `undefined`; the caller then invoked `.apply` on that `undefined` and
//! failed several frames away with a misleading "Function.prototype.apply was called
//! on a value that is not a function". mysql2's row-parser codegen is exactly this
//! shape — `Function.apply(null, argNames.concat(body)).apply(null, argValues)` —
//! so a real MySQL query died with an error naming neither eval nor the real cause.
//!
//! Literal-source forms must keep working: those are const-folded and compiled AOT.

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

/// Build `libperry_{runtime,stdlib}.a` once so the compiled binaries can link.
/// `perry-runtime` / `perry-stdlib` are rlib-only (#5422) — the archives come from the
/// `-static` wrapper crates, so building the plain crates leaves no `.a` in
/// `target/debug` and the link silently falls back to whatever stale archive is around.
fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime-static")
            .arg("-p")
            .arg("perry-stdlib-static")
            .output()
            .expect("run cargo build -p perry-runtime-static -p perry-stdlib-static");
        assert!(
            build.status.success(),
            "cargo build of the static runtime wrappers failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

/// Literal-source `Function` / `Function.apply` / `Function.call` are const-folded and
/// compiled; only the runtime-assembled argument list is a deferred eval surface. The
/// `mysql2` arm mirrors that library's codegen: build the arg list with `.concat()`,
/// hand it to `Function.apply`, then `.apply` the result — the shape that used to
/// surface as "Function.prototype.apply was called on a value that is not a function".
const MAIN_FIXTURE: &str = r#"
const add: any = Function("a", "b", "return a + b;");
console.log("LITERAL_FN:" + add(1, 2));

const sub: any = Function.apply(null, ["a", "b", "return a - b;"]);
console.log("LITERAL_APPLY:" + sub(9, 4));

const mul: any = Function.call(null, "a", "b", "return a * b;");
console.log("LITERAL_CALL:" + mul(3, 4));

if (process.argv.indexOf("--dynamic") !== -1) {
  const body: string = ["return", "a", "+", "100;"].join(" ");
  const argNames: string[] = ["a"];
  try {
    const compiled: any = Function.apply(null, argNames.concat(body));
    const r = compiled.apply(null, [1]);
    console.log("NO_THROW:" + r);
  } catch (e: any) {
    console.log("CAUGHT:" + (e && e.message));
  }
}
console.log("DONE");
"#;

fn write_fixture(root: &std::path::Path) {
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
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .env("PERRY_NO_AUTO_OPTIMIZE", "1");
    for a in extra_args {
        cmd.arg(a);
    }
    cmd.output().expect("run perry compile")
}

#[test]
fn function_apply_with_runtime_args_defers_to_a_located_aot_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_fixture(root);

    let out = compile(root, &[]);
    assert!(
        out.status.success(),
        "compilation must succeed (deferred, not refused)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let bin = root.join("main_bin");

    // Never reaching the dynamic site: the program runs, and the const-foldable
    // literal-source forms still compile and evaluate exactly as in node.
    let run = Command::new(&bin).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "binary must run when the dynamic site is not reached"
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        stdout.contains("LITERAL_FN:3")
            && stdout.contains("LITERAL_APPLY:5")
            && stdout.contains("LITERAL_CALL:12")
            && stdout.contains("DONE"),
        "literal-source Function/apply/call must still be compiled AOT; got:\n{stdout}"
    );

    // Reaching it throws a catchable, descriptive Error — NOT `undefined` flowing on
    // into a bogus "apply was called on a value that is not a function".
    let run2 = Command::new(&bin)
        .arg("--dynamic")
        .output()
        .expect("run compiled binary --dynamic");
    assert!(
        run2.status.success(),
        "the binary must not crash when the dynamic Function site is reached"
    );
    let stdout2 = String::from_utf8_lossy(&run2.stdout);
    assert!(
        stdout2.contains("CAUGHT:"),
        "the runtime-assembled Function.apply must throw a catchable Error; got:\n{stdout2}"
    );
    assert!(
        stdout2.contains("cannot run in an ahead-of-time compiled binary"),
        "the thrown Error must name the AOT limitation; got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("was called on a value that is not a function"),
        "must NOT degrade into `undefined` and fail later inside `.apply`; got:\n{stdout2}"
    );
    assert!(
        !stdout2.contains("NO_THROW"),
        "the dynamic Function site must not silently produce a value; got:\n{stdout2}"
    );
}
