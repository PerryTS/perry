//! Regression test for #5982 (a #5466 representation-lowering regression):
//! a closure capturing a MODULE-LEVEL `const` bound to a typed value read
//! the wrong slot.
//!
//! `for (let i…) { const c = i; fns.push(() => c); }` returned `0,0,0,0,0`
//! instead of `0,1,2,3,4`. The captured module-level `c` is read by the
//! closure through `@perry_global_*` (module globals are filtered out of the
//! capture array), but its declared numeric type made the typed-ABI closure
//! specialization read `js_closure_get_capture_bits(this, 0)` — an unset slot
//! (0) — and the dispatcher picked that typed body. Module-global captures no
//! longer feed the type-directed unboxed capture path.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, src: &str) -> String {
    let entry = dir.join("main.ts");
    let out = dir.join("main_bin");
    std::fs::write(&entry, src).expect("write");
    let c = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .arg("--no-cache")
        .output()
        .expect("compile");
    assert!(
        c.status.success(),
        "compile failed\n{}",
        String::from_utf8_lossy(&c.stderr)
    );
    let r = Command::new(&out).current_dir(dir).output().expect("run");
    assert!(
        r.status.success(),
        "run failed\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).into_owned()
}

#[test]
fn loop_captured_module_const_reads_own_iteration_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
const fns: Array<() => number> = [];
for (let i = 0; i < 5; i++) {
    const captured = i;
    fns.push(() => captured);
}
console.log(fns[0](), fns[1](), fns[2](), fns[3](), fns[4]());
"#,
    );
    assert_eq!(out, "0 1 2 3 4\n");
}

#[test]
fn loop_direct_capture_of_let_var_still_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = compile_and_run(
        dir.path(),
        r#"
const fns: Array<() => number> = [];
for (let i = 0; i < 5; i++) {
    fns.push(() => i);
}
console.log(fns[0](), fns[1](), fns[2](), fns[3](), fns[4]());
"#,
    );
    assert_eq!(out, "0 1 2 3 4\n");
}
