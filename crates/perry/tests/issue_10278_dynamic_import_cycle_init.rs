//! Regression test for #10278: a module reached only through a dynamic
//! `import()` (`ModuleInitKind::Deferred`) that takes part in an import cycle
//! must still initialize its cycle partner.
//!
//! `module_init_deps` (run_pipeline) drops init-call back-edges so that an
//! Eager module's `__init` wrapper never re-enters a cycle member the entry's
//! eager init loop has already run — that is #6463, and dropping the edge is
//! only sound *because* that loop exists. Deferred modules are filtered out of
//! it (`codegen/entry.rs`), so nothing else runs them: dropping a wrapper's
//! edge to a Deferred dep strands the dep forever. Its body never runs and
//! every export the body assigns at runtime stays undefined, which surfaces as
//! `TypeError: value is not a function` on the first call through such a slot.
//!
//! The two tests are deliberately a pair. `dynamic` is the defect: it fails on
//! the pre-fix compiler with `assigned-UNDEFINED`. `static_control` pins the
//! #6463 ordering it must not disturb — the same cycle entered statically,
//! where the eager loop is what initializes both members.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            perry_bin()
                .parent()
                .expect("compiler directory")
                .to_path_buf()
        })
}

/// Cycle member whose body assigns two exports at run time. A function
/// declaration alone would not witness the bug: those are reachable without the
/// body ever running.
const CYCLE_A: &str = "\
console.log(\"[init] a body start\");
import { bFn } from \"./b.js\";
export let assigned;
assigned = () => \"a-assigned-ok\";
export const obj = {};
obj.method = () => \"a-obj-method-ok\";
export function aFn() { return \"a:\" + bFn(); }
console.log(\"[init] a body end\");
";

const CYCLE_B: &str = "\
console.log(\"[init] b body start\");
import { aFn, assigned, obj } from \"./a.js\";
export function bFn() { return \"b\"; }
export function useA() { return aFn(); }
export function useAssigned() { return assigned ? assigned() : \"assigned-UNDEFINED\"; }
export function useObj() {
  return typeof obj.method === \"function\" ? obj.method() : \"obj-method-NOT-A-FUNCTION\";
}
console.log(\"[init] b body end\");
";

const DYNAMIC_ENTRY: &str = "\
const m = await import(\"./b.js\");
console.log(m.bFn(), m.useA(), m.useAssigned(), m.useObj());
";

const STATIC_ENTRY: &str = "\
import { bFn, useA, useAssigned, useObj } from \"./b.js\";
console.log(bFn(), useA(), useAssigned(), useObj());
";

/// Node and bun print the cycle partner's body first in both entry shapes.
const EXPECTED: &str = "\
[init] a body start
[init] a body end
[init] b body start
[init] b body end
b a:b a-assigned-ok a-obj-method-ok
";

fn compile_and_run(entry_src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("a.js"), CYCLE_A).unwrap();
    std::fs::write(root.join("b.js"), CYCLE_B).unwrap();
    std::fs::write(root.join("entry.js"), entry_src).unwrap();

    let entry = root.join("entry.js");
    let output = root.join("entry_bin");
    let out = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-codegen")
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "import cycle must compile; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled cycle binary must run; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8(run.stdout).expect("UTF-8 stdout")
}

#[test]
fn dynamic_import_into_cycle_initializes_the_partner() {
    let stdout = compile_and_run(DYNAMIC_ENTRY);
    assert!(
        stdout.contains("[init] a body start"),
        "the cycle partner's body must run when the cycle is entered through \
         a dynamic import; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("assigned-UNDEFINED") && !stdout.contains("obj-method-NOT-A-FUNCTION"),
        "run-time-assigned exports of the cycle partner must be live; stdout:\n{stdout}"
    );
    assert_eq!(
        stdout, EXPECTED,
        "dynamic-import cycle output must match node/bun byte-for-byte"
    );
}

#[test]
fn static_import_into_cycle_keeps_the_6463_order() {
    let stdout = compile_and_run(STATIC_ENTRY);
    assert_eq!(
        stdout, EXPECTED,
        "static-import cycle output must match node/bun byte-for-byte"
    );
}
