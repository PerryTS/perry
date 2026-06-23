//! Regression test for #5579: a STRING-keyed write to an `arguments` object
//! through an untyped receiver (`obj[name] = v` where `obj`/`name` are plain
//! `any` parameters) must set the *named* property — not clobber element 0.
//!
//! Such writes lower to the runtime `js_dyn_index_set` dispatcher. That helper
//! coerced any non-numeric index to `0` (`index.is_nan() -> idx_i32 = 0`) and
//! then unconditionally tried `arguments_object_set_index(obj, 0, value)`, so
//! `args["gp"] = v` on an arguments object wrote `args[0]` and silently dropped
//! the named property. #5544 widened unknown-receiver string-key writes onto
//! this path, which surfaced ~33 newly-failing `built-ins/Object/define
//! Propert{y,ies}` test262 cases (their propertyHelper `isWritable(args, name)`
//! probe does exactly `args[name] = v` with an untyped `name`, then re-reads
//! the property and saw the stale value -> "descriptor should be writable").
//!
//! The fix mirrors the IndexGet string-index arm: a string index routes through
//! `js_object_set_field_by_name`, which honours arrays / arguments objects /
//! plain objects with the correct descriptor + writable semantics. This test
//! pins the behaviour without a test262 checkout, so it runs in CI.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn arguments_string_key_write_through_untyped_receiver() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");

    std::fs::write(
        &entry,
        r#"
// Untyped receiver + untyped key — the exact shape test262 propertyHelper's
// isWritable(obj, name) uses: `obj[name] = v` reaches js_dyn_index_set with a
// NaN-boxed string index.
function set(obj: any, name: any, v: any): void { obj[name] = v; }
function get(obj: any, name: any): any { return obj[name]; }

// (A) defineProperty'd writable own property on `arguments`: the named write
// must update "gp" and leave element 0 (the first parameter) untouched.
(function () {
  Object.defineProperty(arguments, "gp", {
    value: 1001, writable: true, enumerable: true, configurable: true,
  });
  set(arguments, "gp", "X");
  console.log("A=" + arguments[0] + "," + (arguments as any)["gp"]);
}(1, 2, 3));

// (B) verifyProperty-style writable probe: read back through getOwnPropertyDescriptor.
(function () {
  Object.defineProperty(arguments, "p", {
    value: 7, writable: true, enumerable: true, configurable: true,
  });
  set(arguments, "p", "Y");
  const d = Object.getOwnPropertyDescriptor(arguments, "p");
  console.log("B=" + d!.value + "," + get(arguments, "p"));
}(10, 20));

// (C) a brand-new named property created via the dynamic write must persist.
(function () {
  set(arguments, "fresh", 99);
  console.log("C=" + arguments[0] + "," + (arguments as any)["fresh"]);
}(5, 6));

// (D) numeric string-keyed write to arguments still addresses the element.
(function () {
  set(arguments, "0", "Z");
  console.log("D=" + arguments[0]);
}(40, 41));

// (E) plain objects / arrays unaffected (no regression).
const o: any = {};
Object.defineProperty(o, "k", { value: 1, writable: true, configurable: true });
set(o, "k", "OK");
const a: any = [1, 2, 3];
set(a, "named", "NM");
console.log("E=" + o.k + "," + a.named + "," + a[0]);
"#,
    )
    .expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        "A=1,X\n\
         B=Y,Y\n\
         C=5,99\n\
         D=Z\n\
         E=OK,NM,1\n",
        "a string-keyed dynamic write to an arguments object must set the named \
         property (not clobber element 0); node output is the oracle"
    );
}
