//! Regression test for #5587: subclassing a `Temporal.<Type>` constructor must
//! produce an instance that carries the Temporal brand, so the inherited
//! prototype methods / accessor getters dispatch and `instanceof` resolves.
//!
//! This is the dominant failure mode behind the test262
//! `built-ins/Temporal/**/subclassing-ignored.js` cluster, whose
//! `TemporalHelpers.checkSubclassingIgnored` helper does:
//! ```js
//! class MySubclass extends construct { constructor() { super(...args); } }
//! const instance = new MySubclass();
//! const result = instance[method](...methodArgs);   // <- threw pre-fix
//! assert.sameValue(Object.getPrototypeOf(result), construct.prototype);
//! ```
//!
//! Pre-fix, `super(...)` to a native Temporal constructor went through
//! `js_fetch_or_value_super`'s generic implicit-`this`-bound dispatch, which
//! ran the native ctor (returning a fresh Temporal cell) but DISCARDED that
//! cell — leaving the subclass instance an empty plain object with no brand. So
//! `instance.abs()` resolved `abs` to `undefined` and threw
//! `TypeError: value is not a function`.
//!
//! Fix (perry-runtime): `js_fetch_or_value_super` now detects a Temporal parent
//! constructor, runs it, and stashes the returned cell on `this` under
//! `__perry_temporal_cell__`; method-call / getter / instanceof dispatch
//! recover the cell from there.

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
        "compiled binary failed (pre-fix: 'TypeError: value is not a \
         function')\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn temporal_duration_subclass_dispatches_and_ignores_subclassing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const constructArgs = [0, 0, 0, -4, -5, -6, -7, -987, -654, -321];

// Mirror TemporalHelpers.checkSubclassingIgnored's MySubclass shape.
let called = 0;
class MySubclass extends Temporal.Duration {
  constructor() {
    ++called;
    super(...constructArgs);
  }
}

const instance = new MySubclass();
console.log("called:", called);                       // 1
console.log("instanceof:", instance instanceof Temporal.Duration);  // true
console.log("days:", (instance as any).days);         // -4 (inherited getter)

// The method under test returns a BASE Temporal.Duration, never a subclass.
const result = (instance as any).abs();
console.log("result-proto:",
  Object.getPrototypeOf(result) === Temporal.Duration.prototype);   // true
console.log("result-not-sub:",
  Object.getPrototypeOf(result) !== MySubclass.prototype);          // true
console.log("abs-days:", result.days);                // 4
console.log("abs-str:", result.toString());           // P4DT5H6M7.987654321S

// Computed-key method call (the helper uses instance[method](...)).
const m = "negated";
const neg = (instance as any)[m]();
console.log("neg-days:", neg.days);                   // 4

// The subclass constructor is called exactly once across all of the above.
console.log("called-final:", called);                 // 1
"#,
    );
    assert_eq!(
        stdout,
        "called: 1\n\
         instanceof: true\n\
         days: -4\n\
         result-proto: true\n\
         result-not-sub: true\n\
         abs-days: 4\n\
         abs-str: P4DT5H6M7.987654321S\n\
         neg-days: 4\n\
         called-final: 1\n"
    );
}

#[test]
fn temporal_duration_subclass_via_aliased_heritage() {
    // `class X extends D` where `D` is an alias of `Temporal.Duration`. The
    // non-spread `super(...)` path must recover the Temporal parent even when the
    // immediate heritage value arrives stale, via the decl-time parent stash.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
const D = Temporal.Duration;
class X extends D {
  constructor() {
    super(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
  }
}
const x = new X();
console.log("instanceof:", x instanceof Temporal.Duration);   // true
console.log("days:", (x as any).days);                        // 4
console.log("abs-proto:",
  Object.getPrototypeOf((x as any).abs()) === Temporal.Duration.prototype);  // true
"#,
    );
    assert_eq!(
        stdout,
        "instanceof: true\n\
         days: 4\n\
         abs-proto: true\n"
    );
}

#[test]
fn temporal_plain_date_subclass_dispatches() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class MyDate extends Temporal.PlainDate {
  constructor() {
    super(2021, 7, 20);
  }
}

const d = new MyDate();
console.log("instanceof:", d instanceof Temporal.PlainDate);   // true
console.log("year:", (d as any).year);                         // 2021
console.log("month:", (d as any).month);                       // 7
console.log("day:", (d as any).day);                           // 20

// add() returns a base PlainDate, not the subclass.
const later = (d as any).add({ days: 12 });
console.log("add-proto:",
  Object.getPrototypeOf(later) === Temporal.PlainDate.prototype);  // true
console.log("add-day:", later.day);                            // 1 (Aug 1)
console.log("add-month:", later.month);                        // 8
"#,
    );
    assert_eq!(
        stdout,
        "instanceof: true\n\
         year: 2021\n\
         month: 7\n\
         day: 20\n\
         add-proto: true\n\
         add-day: 1\n\
         add-month: 8\n"
    );
}
