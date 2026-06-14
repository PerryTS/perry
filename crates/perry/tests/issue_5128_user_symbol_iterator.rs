//! Regression test for #5128: a user-defined class with a generator
//! `*[Symbol.iterator]()` method was not recognized as iterable — spreading it
//! (`[...x]`), `Math.max(...x)`, `Array.from(x)`, destructuring, and a manual
//! `x[Symbol.iterator]()` all threw `TypeError: value is not iterable`.
//!
//! Root cause: a generator `[Symbol.iterator]` method is lifted to a top-level
//! `__perry_iter_<Class>` function that the syntactic `for…of` fast path calls
//! directly. But the class itself carried no `@@iterator` method, so every
//! *runtime*-dispatched iterator consumer (which resolves `Symbol.iterator`
//! through the class registry) found nothing.
//!
//! Fix (three parts):
//!  - HIR: also register a synthetic non-generator `@@iterator` class method
//!    that forwards to the lifted generator (`return __perry_iter_X(this)`).
//!  - Runtime: `js_object_get_symbol_property` maps the well-known
//!    `Symbol.iterator` / `Symbol.asyncIterator` to the `@@iterator` /
//!    `@@asyncIterator` class method and returns a bound method.
//!  - Runtime: the array-spread path sets `IMPLICIT_THIS` around the iterator
//!    call so the bound `@@iterator` wrapper sees the right receiver.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (pre-fix: 'TypeError: value is not iterable')\n\
         status: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn class_generator_symbol_iterator_is_iterable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Range {
  lo: number; hi: number;
  constructor(lo: number, hi: number) { this.lo = lo; this.hi = hi; }
  *[Symbol.iterator]() { for (let i = this.lo; i <= this.hi; i++) yield i; }
}
console.log([...new Range(1, 5)].join(","));   // spread
console.log(Math.max(...new Range(1, 5)));      // Math.max spread
let s = 0; for (const x of new Range(1, 5)) s += x; console.log(s); // for-of
const r = new Range(1, 3);
console.log([...r].join(","));                  // spread via variable
const [a, b, c] = new Range(10, 12); console.log(a, b, c); // destructuring
console.log(Array.from(new Range(1, 4)).join(",")); // Array.from
const it = new Range(7, 8)[Symbol.iterator]();  // manual iterator
console.log(it.next().value, it.next().value, it.next().done);
"#,
    );
    assert_eq!(
        stdout,
        "1,2,3,4,5\n5\n15\n1,2,3\n10 11 12\n1,2,3,4\n7 8 true\n"
    );
}

/// A non-generator `[Symbol.iterator]()` method (delegating to a backing
/// array's iterator) must also be reachable by runtime consumers, and plain
/// array/string spreads must keep working.
#[test]
fn class_plain_symbol_iterator_and_spread_regression() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class Wrap { data = [9, 8, 7]; [Symbol.iterator]() { return this.data[Symbol.iterator](); } }
console.log([...new Wrap()].join(","));
console.log([...[1, 2, 3], ...[4, 5]].join(","));
console.log([..."abc"].join("-"));
"#,
    );
    assert_eq!(stdout, "9,8,7\n1,2,3,4,5\na-b-c\n");
}
