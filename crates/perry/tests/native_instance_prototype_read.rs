//! Regression test: reading object-metadata properties (`prototype`,
//! `__proto__`, `constructor`) on a value the HIR tagged as a *native
//! instance* must be a property GET, not an invoking native method call.
//!
//! `const lN = Readable.from([...])` tags `lN` as a `stream`/`Readable`
//! native instance. Pre-fix, `lN.prototype` fell through to the 0-arg
//! `NativeMethodCall` fallback, lowering to
//! `js_native_call_method_nullsafe(lN, "prototype", 0 args)` — which
//! *invokes* the resolved prototype value → `TypeError: prototype is not a
//! function`. Node returns the real metadata (`undefined` for an instance's
//! `.prototype`).
//!
//! This is the wall that blocked `claude-code --help` (the bundled
//! follow-redirects `RedirectableRequest` hits the same native-instance
//! mis-classification when reading `lN.prototype` to attach methods).

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn native_instance_prototype_read_is_a_property_get() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
import { Readable } from "stream";
const lN: any = Readable.from(["a", "b"]);
// `.prototype` / `.constructor` / `.__proto__` are metadata reads, not calls.
console.log("prototype:", typeof lN.prototype);
console.log("constructor-is-fn:", typeof lN.constructor === "function");
console.log("ok");
"#,
    );
    assert_eq!(
        stdout, "prototype: undefined\nconstructor-is-fn: true\nok\n",
        "metadata property reads on a native instance must not be invoked"
    );
}
