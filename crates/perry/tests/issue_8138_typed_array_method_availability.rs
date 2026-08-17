//! Regression tests for #8138: Array-only methods must not resolve on
//! `%TypedArray%` instances.

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

fn runtime_dir() -> PathBuf {
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
            .expect("build static runtime archives");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });

    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        // Windows' RS4GC path cannot lower the try/catch assertions in this
        // fixture yet (#7354); this issue is independent of native roots.
        .env("PERRY_RS4GC", "0")
        // Link the archives built from this worktree, not an auto-optimized
        // runtime cache that may predate the fix.
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn array_only_methods_throw_on_typed_arrays_without_mutating_them() {
    let stdout = compile_and_run(
        r#"
function opaque(value: any): any { return value; }

function report(name: string, array: Int32Array, action: () => any) {
  let threw = false;
  let errorName = "none";
  try {
    action();
  } catch (error: any) {
    threw = true;
    errorName = error.name;
  }
  console.log(name + ":" + threw + ":" + errorName + ":" + array.join(","));
}

const flat = new Int32Array([1, 2, 3]);
report("flat", flat, () => (flat as any).flat());

const flatMap = new Int32Array([1, 2, 3]);
report("flatMap", flatMap, () => (flatMap as any).flatMap((x: number) => [x]));

const push = new Int32Array([1, 2, 3]);
report("push", push, () => (push as any).push(9));

const pop = new Int32Array([1, 2, 3]);
report("pop", pop, () => (pop as any).pop());

const shift = new Int32Array([1, 2, 3]);
report("shift", shift, () => (shift as any).shift());

const unshift = new Int32Array([1, 2, 3]);
report("unshift", unshift, () => (unshift as any).unshift(9));

const splice = new Int32Array([1, 2, 3]);
report("splice", splice, () => (splice as any).splice(1, 1));

const toSpliced = new Int32Array([1, 2, 3]);
report("toSpliced", toSpliced, () => (toSpliced as any).toSpliced(1, 1));

const dynamic = new Int32Array([1, 2, 3]);
report("dynamic-flatMap", dynamic, () => opaque(dynamic).flatMap((x: number) => [x]));

const own = new Int32Array([1, 2, 3]);
(own as any).flat = () => 42;
console.log("own-flat:" + (own as any).flat() + ":" + own.join(","));

(Int32Array.prototype as any).flat = () => 43;
const inherited = new Int32Array([1, 2, 3]);
console.log("prototype-flat:" + (inherited as any).flat() + ":" + inherited.join(","));

const control = new Int32Array([1, 2, 3]);
report("control", control, () => { throw new TypeError("control"); });
"#,
    );

    for name in [
        "flat",
        "flatMap",
        "push",
        "pop",
        "shift",
        "unshift",
        "splice",
        "toSpliced",
        "dynamic-flatMap",
        "control",
    ] {
        let expected = format!("{name}:true:TypeError:1,2,3");
        assert!(
            stdout.lines().any(|line| line.trim() == expected),
            "{name} must throw TypeError without changing the receiver (got:\n{stdout})"
        );
    }
    assert!(stdout
        .lines()
        .any(|line| line.trim() == "own-flat:42:1,2,3"));
    assert!(stdout
        .lines()
        .any(|line| line.trim() == "prototype-flat:43:1,2,3"));
}
