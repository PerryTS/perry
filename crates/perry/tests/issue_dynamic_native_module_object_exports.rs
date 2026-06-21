//! Dynamic native-module namespaces — those produced by `createRequire(...)(spec)`
//! and `process.getBuiltinModule(spec)` — must dispatch OBJECT-VALUED exports
//! (e.g. `perf_hooks.performance` / `perf_hooks.constants`), not only methods
//! and constructors.
//!
//! Before the fix, `vt_get_own_field` resolved overrides / constants / callable
//! exports but returned `undefined` for object-valued exports, while the static
//! codegen path resolved them via `js_native_module_property_by_name`. So
//! `createRequire(...)("perf_hooks").performance` was `undefined` and a later
//! `.performance.mark(...)` threw `Cannot read properties of undefined (reading
//! 'mark')`. The fix delegates the fall-through to the authoritative resolver,
//! so dynamic namespaces match the static path (and preserve singleton identity).

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
fn dynamic_native_module_namespace_dispatches_object_valued_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { createRequire } from "module";
const req: any = createRequire(import.meta.url);
const ph: any = req("perf_hooks");

// Object-valued exports must resolve (not undefined), and methods/ctors still work.
console.log("performance:", typeof ph.performance);
console.log("perf.mark:", typeof ph.performance?.mark);
console.log("constants:", typeof ph.constants);
console.log("now:", typeof ph.now, "PerformanceObserver:", typeof ph.PerformanceObserver);

// A real call through the object-valued export must not throw.
ph.performance.mark("test-mark");
console.log("mark-called: ok");

// Singleton identity: the dynamic and static perf objects are the same object.
const g: any = require("perf_hooks");
console.log("identity:", ph.performance === g.performance);

// node: prefix form resolves the same way.
const ph2: any = req("node:perf_hooks");
console.log("node-prefix performance:", typeof ph2.performance);
console.log("DONE");
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "performance: object",
        "perf.mark: function",
        "constants: object",
        "now: function PerformanceObserver: function",
        "mark-called: ok",
        "identity: true",
        "node-prefix performance: object",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
