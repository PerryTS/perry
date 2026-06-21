//! `class X extends Array { constructor(n){ super(n); this.fill(v) } }` —
//! subclassing the builtin `Array`. perry models the subclass instance as a
//! plain object, not a real exotic Array (`ArrayHeader` has no `class_id`
//! slot), so `super(n)` used to leave it length-less with no Array methods and
//! `this.fill(0)` threw `TypeError: value is not a function`.
//!
//! Fix: the `super()` lowering for an `Array` parent calls a runtime
//! `js_array_subclass_init(this, n)` that sets a visible `length = ToLength(n)`
//! and installs the Array surface the instance relies on (`fill`), delegating
//! to the generic array-like impl (`js_array_fill_generic`, which operates on
//! the receiver's own `length` + indexed properties). Indexed get/set already
//! work as ordinary object properties. Mirrors the EventEmitter subclass-init
//! pattern. lru-cache's `ZeroArray` is the motivating shape.

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
fn array_subclass_super_sizes_and_fill_works() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(
        &entry,
        r#"
// lru-cache's ZeroArray shape: a fixed-size zero-initialised indexed buffer.
class Zero extends Array {
  constructor(n: number) {
    super(n)
    this.fill(0) // must NOT throw "value is not a function"
  }
}

const z: any = new Zero(4)
console.log("length:", z.length)            // super(n) sizes the instance
console.log("filled:", z[0], z[1], z[2], z[3]) // fill(0) zero-initialised slots
z[2] = 9                                     // indexed write
console.log("after set:", z[2])
console.log("typeof fill:", typeof z.fill)

// fill with a non-zero value on a second instance
const z2: any = new Zero(3)
z2.fill(7)
console.log("z2:", z2[0], z2[1], z2[2])
console.log("DONE")
"#,
    )
    .expect("write entry");

    let (ok, stdout) = compile_and_run(dir.path(), &entry);
    assert!(ok, "binary failed\nstdout:\n{stdout}");
    for needle in [
        "length: 4",
        "filled: 0 0 0 0",
        "after set: 9",
        "typeof fill: function",
        "z2: 7 7 7",
        "DONE",
    ] {
        assert!(
            stdout.contains(needle),
            "expected `{needle}` in output:\n{stdout}"
        );
    }
}
