//! Regression test for #2768 — `Reflect.construct` newTarget validation.
//!
//! Two bugs, fixed together:
//!  1. The known-class fold (`Reflect.construct(C, [args]) → new C(args)` in
//!     `lower/expr_call/native_module.rs`) also fired for the three-argument
//!     form, folding `Reflect.construct(C, [args], newTarget)` to a plain
//!     `new C(args)` — silently dropping `newTarget` (both its prototype and
//!     its constructor validation). Now only the two-argument form folds; with
//!     an explicit `newTarget` it falls through to runtime `js_reflect_construct`.
//!  2. `is_constructor_function` (proxy.rs) checked only the builtin
//!     non-constructable set, not `closure_is_arrow`, so an arrow `newTarget`
//!     passed validation. Now arrow functions are rejected, so
//!     `Reflect.construct(C, args, arrowFn)` throws a TypeError like Node.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_ts(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write");
    let out = dir.path().join("bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&out).output().expect("run");
    assert!(
        run.status.success(),
        "compiled binary exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

#[test]
fn reflect_construct_new_target_prototype_and_validation() {
    let stdout = run_ts(
        r#"
class A { x: number; constructor(a: number, b: number) { this.x = a + b } }
class B {}
const arrow = () => {}

// 1. valid newTarget: result inherits newTarget's prototype, runs A's body.
const o: any = Reflect.construct(A, [2, 3], B)
console.log("proto_is_B:", Object.getPrototypeOf(o) === B.prototype)
console.log("x:", o.x)

// 2. arrow newTarget is not a constructor -> TypeError.
let threw = false
try { Reflect.construct(A, [1, 1], arrow as any) } catch (e: any) { threw = e instanceof TypeError }
console.log("arrow_newtarget_throws:", threw)

// 3. arrow target is not a constructor -> TypeError (regression guard).
let threwTarget = false
try { Reflect.construct(arrow as any, []) } catch (e: any) { threwTarget = e instanceof TypeError }
console.log("arrow_target_throws:", threwTarget)

// 4. two-arg fold still works, and array-like argumentsList is accepted.
const basic: any = Reflect.construct(A, [4, 5])
console.log("basic_x:", basic.x, "is_A:", basic instanceof A)
const al: any = Reflect.construct(A, { 0: 10, 1: 20, length: 2 } as any)
console.log("arraylike_x:", al.x)
"#,
    );

    for needle in [
        "proto_is_B: true",
        "x: 5",
        "arrow_newtarget_throws: true",
        "arrow_target_throws: true",
        "basic_x: 9 is_A: true",
        "arraylike_x: 30",
    ] {
        assert!(stdout.contains(needle), "missing `{needle}` in:\n{stdout}");
    }
}
