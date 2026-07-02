//! Regression: `delete <primitive>.field` must be a no-op evaluating to `true`,
//! not unbox the primitive's bits as an `ObjectHeader*` and crash.
//!
//! Surfaced by an esbuild `__esm` zod-style schema-init shape:
//! `function Fq(q){ let K=q; ...; if(delete K.message, typeof K.error==="string")... }`
//! invoked with a non-object `q` (a number). `RequireObjectCoercible` passes for
//! a number (numbers ARE coercible), so the codegen unboxed the number to a
//! garbage `ObjectHeader*` and `js_object_delete_field` faulted (EXC_BAD_ACCESS)
//! reading the kind byte at `[ptr-8]`. Per spec, `delete (5).message` ToObjects
//! the primitive to a throwaway wrapper whose property is not own → returns
//! `true`, no mutation. Fixed via `js_object_delete_field_value` /
//! `js_object_delete_dynamic_value`, which no-op for non-pointer receivers.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String) {
    let output = dir.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(entry)
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
    (
        run.status.success(),
        String::from_utf8_lossy(&run.stdout).to_string(),
    )
}

#[test]
fn delete_on_primitive_receiver_is_noop_true() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
let n: any = 5;
console.log("num.static:", delete n.message);
console.log("num.dynamic:", delete n["message"]);
let b: any = true;
console.log("bool.static:", delete b.foo);
let s: any = "hello";
console.log("str.static:", delete s.nope);

// Real-object delete still reaches the runtime path (returns true).
let o: any = {};
o.x = 1;
console.log("obj.dynamic:", delete o.x);

// The exact crash shape: `delete K.message` where K is a non-object primitive.
function Fq(q: any) {
  let K = q;
  if (delete K.message, typeof K.error === "string") return "err";
  return "ok";
}
console.log("Fq(5):", Fq(5));
console.log("Fq(obj):", Fq({ error: "x" }));
console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, out) = compile_and_run(dir.path(), &entry);
    assert!(
        ok,
        "compiled binary did not exit cleanly (delete-on-primitive crash regressed)\nstdout:\n{out}"
    );
    assert!(
        out.contains("num.static: true"),
        "delete num.field → true\n{out}"
    );
    assert!(
        out.contains("num.dynamic: true"),
        "delete num[k] → true\n{out}"
    );
    assert!(
        out.contains("bool.static: true"),
        "delete bool.field → true\n{out}"
    );
    assert!(
        out.contains("str.static: true"),
        "delete str.field → true\n{out}"
    );
    assert!(
        out.contains("obj.dynamic: true"),
        "delete obj.dynProp → true\n{out}"
    );
    assert!(out.contains("Fq(5): ok"), "Fq(5) must not crash\n{out}");
    assert!(
        out.contains("Fq(obj): err"),
        "Fq(object) still reads error\n{out}"
    );
    assert!(
        out.contains("DONE"),
        "program must run to completion\n{out}"
    );
}
