//! An aliased named import of `setTimeout` from `node:timers/promises` must
//! NOT shadow the global `setTimeout(callback, delay)`.
//!
//! `import { setTimeout as delayP } from "node:timers/promises"` binds only the
//! local name `delayP`; the bare identifier `setTimeout` still refers to the
//! GLOBAL callback-first timer. A prior version of the submodule-import
//! registration keyed the routing map by BOTH the local alias AND the imported
//! name whenever they differed, so the global `setTimeout(() => {}, ms)` was
//! diverted to the delay-first `timers/promises` thunk. That thunk validated
//! the callback as the `delay` argument and rejected the promise with
//! `TypeError: The "delay" argument must be of type number. Received function`
//! — an uncaught rejection (observed via a CLI bundle's `mcp list` path).
//!
//! Keying the map only by the local alias keeps the alias routed to the
//! promises export while leaving the unshadowed global intact. The unaliased
//! `import { setTimeout } from "node:timers/promises"` form (which DOES shadow
//! the global, matching Node) is unaffected.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry: &std::path::Path) -> (bool, String, String) {
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
        String::from_utf8_lossy(&run.stderr).to_string(),
    )
}

#[test]
fn aliased_timers_promises_set_timeout_does_not_shadow_global() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { setTimeout as delayP } from "node:timers/promises";

(async () => {
  // The aliased promises form: delay-first, returns a Promise.
  await delayP(0);
  console.log("promises-form ok");

  // The GLOBAL callback-first form must still work in the same module.
  setTimeout(() => { console.log("global cb fired"); }, 0);
  console.log("global scheduled ok");
})();
"#,
    )
    .expect("write entry");

    let (ok, stdout, stderr) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        !stderr.contains("The \"delay\" argument must be of type number"),
        "global setTimeout was wrongly routed to the timers/promises validator\nstderr:\n{stderr}"
    );
    for needle in ["promises-form ok", "global scheduled ok", "global cb fired"] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
