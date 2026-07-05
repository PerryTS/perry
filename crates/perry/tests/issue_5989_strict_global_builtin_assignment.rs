//! Regression (#5989): strict-mode assignment to an identifier with no
//! lexical binding must resolve against the global object per spec
//! (PutValue): an EXISTING global property — `Date = wrapped` — is a
//! normal property write, and only a genuinely absent binding throws the
//! ReferenceError.
//!
//! Next.js 16's `cacheComponents` node-environment extensions install a
//! `Date` class extension exactly this way (strict CJS,
//! `Date = createDate(Date)`) so prerenders can detect clock reads as
//! dynamic IO. Perry's old lowering threw unconditionally for any
//! unresolved strict assignment, the install's `catch {}` swallowed it
//! ("Failed to install `Date` class extension" at every server boot),
//! and the dynamic-IO prerender-abort chain never armed — one layer of
//! the dynamic-RSC-route hang tracked in #5989.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(src: &str) -> (bool, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write entry");
    let output = dir.path().join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .args([
            "compile",
            entry.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
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

/// The exact Next.js date.js shape: clone Date's descriptors onto a
/// wrapper, reassign the global `Date` binding in strict code, and
/// observe the wrapper through bare `Date` reads afterwards.
#[test]
fn strict_assignment_to_existing_global_builtin_writes_through() {
    let (ok, stdout) = compile_and_run(
        r#"
"use strict";
let ioCalls = 0;
function io() {
  ioCalls++;
}
function createDate(originalConstructor: DateConstructor) {
  const properties = Object.getOwnPropertyDescriptors(originalConstructor);
  const originalNow = originalConstructor.now;
  properties.now.value = function now() {
    io();
    return originalNow();
  };
  const construct = Reflect.construct;
  const apply = Reflect.apply;
  const newConstructor = Object.defineProperties(function Date1() {
    if (new.target === undefined) {
      io();
      return apply(originalConstructor, undefined, arguments);
    }
    if (arguments.length === 0) {
      io();
    }
    return construct(originalConstructor, arguments, new.target);
  }, properties);
  Object.defineProperty(originalConstructor.prototype, "constructor", {
    value: newConstructor,
  });
  return newConstructor;
}
try {
  // @ts-expect-error deliberate global builtin reassignment
  Date = createDate(Date);
  console.log("install ok");
} catch (err) {
  console.log("install FAILED:", (err as Error)?.message ?? String(err));
}
const d = new Date(0);
console.log("iso:", d.toISOString());
console.log("now is number:", typeof Date.now() === "number");
console.log("instanceof:", d instanceof Date);
// Interception fires through the VALUE path (a captured `Date.now`
// binding routes through the installed wrapper). Syntactic `Date.now()`
// sites still compile to the builtin intrinsic and bypass the override —
// a known, separately-tracked gap.
const capturedNow = Date.now;
capturedNow();
console.log("io calls:", ioCalls);
"#,
    );
    assert!(ok, "run must exit cleanly");
    assert_eq!(
        stdout,
        "install ok\niso: 1970-01-01T00:00:00.000Z\nnow is number: true\ninstanceof: true\nio calls: 1\n",
        "the wrapper must install and intercept a value-path Date.now"
    );
}

/// A genuinely absent binding must still throw the spec ReferenceError
/// in strict mode (with the name in the message).
#[test]
fn strict_assignment_to_absent_global_still_throws_reference_error() {
    let (ok, stdout) = compile_and_run(
        r#"
"use strict";
try {
  // @ts-expect-error deliberate unresolved assignment
  definitelyNotDeclaredAnywhere = 1;
  console.log("no-throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}
"#,
    );
    assert!(ok, "run must exit cleanly");
    assert_eq!(
        stdout,
        "caught: definitelyNotDeclaredAnywhere is not defined\n"
    );
}
