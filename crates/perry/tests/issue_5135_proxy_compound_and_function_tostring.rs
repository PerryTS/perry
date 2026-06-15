//! Regression tests for #5135: importing `immer` and calling
//! `produce(base, draft => { draft.count++; draft.list.push(3) })` crashed with
//! SIGSEGV. immer's drafts are `Proxy` objects that are statically typed as the
//! plain base type, which exposed three independent Perry bugs. These tests
//! reproduce each with a plain `Proxy` (no immer dependency needed):
//!
//!  1. A compound-assignment write through a `Proxy` (`p.count++`) lowered to
//!     `js_object_set_field_by_name` with the proxy's NaN-box tag masked off;
//!     the runtime had no proxy branch there and dereferenced the masked id as
//!     an `ObjectHeader` → SIGSEGV. (The read side already routed proxies.)
//!  2. The statically-typed `Function.toString` static-member read collapsed to
//!     `globalThis.toString` and folded to a number, so
//!     `Function.toString.call(Ctor)` (immer's `isPlainObject`) threw
//!     "Function.prototype.call was called on a value that is not a function".
//!  3. A native array method / `length` read on a value that is a `Proxy` at
//!     runtime (`draft.list.push(x)`) dereferenced the masked proxy id as an
//!     `ArrayHeader` → SIGSEGV. The array helpers now route a proxy receiver
//!     through its traps.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    let output = root.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
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
        "compiled binary failed (signal/exit) — likely a SIGSEGV regression\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

/// Fix #1: `proxy.count++` writes through the `set` trap instead of crashing.
#[test]
fn proxy_compound_assignment_routes_through_set_trap() {
    let out = compile_and_run(
        r#"
const target: any = { count: 0 };
const p: any = new Proxy(target, {
  get(t: any, k: any) { return t[k]; },
  set(t: any, k: any, v: any) { t[k] = v; return true; },
});
p.count++;
console.log(p.count, target.count);
"#,
    );
    assert_eq!(
        out, "1 1\n",
        "p.count++ must write through the proxy set trap"
    );
}

/// Fix #2: `Function.toString` (and `Array.toString`) read as a value are real
/// functions, not numbers.
#[test]
fn function_tostring_static_member_is_callable() {
    let out = compile_and_run(
        r#"
console.log(typeof Function.toString);
console.log(typeof Array.toString);
// immer's isPlainObject reaches `Function.toString.call(Ctor)`:
console.log(typeof Function.toString.call(Array));
"#,
    );
    assert_eq!(
        out, "function\nfunction\nstring\n",
        "Function.toString / Array.toString must resolve to callable functions"
    );
}

/// Fix #3: a native array method (`push`) on a value that is a Proxy at runtime
/// dispatches through the proxy's traps instead of dereferencing the masked
/// proxy id as an ArrayHeader. This mirrors immer's `draft.list.push(x)`, where
/// the receiver is a member access (`obj.list`) that returns a proxy array — the
/// `js_array_push_f64` runtime helper path the issue actually exercised.
#[test]
fn proxy_array_push_via_member_routes_through_traps() {
    let out = compile_and_run(
        r#"
const target: any = [1, 2];
const inner: any = new Proxy(target, {
  get(t: any, k: any) { return t[k]; },
  set(t: any, k: any, v: any) { t[k] = v; return true; },
});
const holder: any = { list: inner };
holder.list.push(3);
console.log(target.join(","), holder.list.length);
"#,
    );
    assert_eq!(
        out, "1,2,3 3\n",
        "obj.list.push must mutate the proxied array through its set trap"
    );
}
