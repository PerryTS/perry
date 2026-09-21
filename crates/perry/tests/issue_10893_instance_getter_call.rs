//! Regression test for #10893: calling the function an INSTANCE getter returns
//! threw `TypeError: <name> is not a function`.
//!
//! `js_native_call_method`'s dispatch tower probed vtable methods, own fields
//! and the prototype chain for a callable VALUE, but never RAN a getter. So a
//! class exposing a callable through `get g()` failed on `c.g(1)` while
//! `const f = c.g; f(1)` returned the very same function — the value was right,
//! only the combined member-call form missed. The runtime even named it:
//! "call-method (no method/field/proto match)".
//!
//! The fix adds an accessor arm at the END of the tower, after every
//! method/field/prototype probe, so a real method of the same name still wins
//! and a getter yielding a non-callable still throws as before.
//!
//! Object-literal getters and plain static getters already worked; they are
//! pinned here so the new arm does not disturb them.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
const obj = { get g() { return (n: number) => n + 1 } }
console.log("1", obj.g(1))

class C { get g() { return (n: number) => n + 2 } }
const c = new C()
console.log("2", c.g(1))
const viaLocal = c.g
console.log("3", viaLocal(1))

class D { static get g() { return (n: number) => n + 3 } }
console.log("4", (D as any).g(1))

// a real method of the same name must still win over any accessor arm
class E { g(n: number) { return n + 100 } }
console.log("5", new E().g(1))

// a getter yielding a non-callable must still throw
class F { get g() { return 42 } }
try {
  ;(new F() as any).g(1)
  console.log("6", "NO THROW")
} catch (e: any) {
  console.log("6", "threw")
}
"#;

/// Byte-for-byte what bun 1.3.14 prints.
const EXPECTED: &str = "1 2\n2 3\n3 3\n4 4\n5 101\n6 threw\n";

#[test]
fn calling_a_function_returned_by_an_instance_getter_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let entry = root.join("main.ts");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let output = root.join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(root)
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
        .current_dir(root)
        .output()
        .expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "pre-fix this threw \"g is not a function\" on line 2\nstdout:\n{}\nstderr:\n{}",
        stdout,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        stdout, EXPECTED,
        "an instance getter's returned function must be callable as `c.g(args)`"
    );
}
