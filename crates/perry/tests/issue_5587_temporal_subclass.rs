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

#[test]
fn temporal_subclass_capture_writeback_inner_class() {
    // Regression for #5587 (Bug 1a + Bug 1b):
    //
    // Bug 1a — stash placement: the `this.__perry_cap_called = param` stash
    // was inserted immediately after `super()`. When user code runs `++called`
    // AFTER `super()` the stash recorded the pre-mutation value 0 and
    // `emit_class_capture_writeback` wrote 0 back to the outer slot.
    // Fix: append stash at END of ctor body (after all user stmts).
    //
    // Bug 1b — inlined-scope ID mismatch: when `check(...)` is called at
    // module level, Perry inlines its body into module-init and alpha-renames
    // locals (`called` id=2 → id=9). The old suffix-based writeback looked
    // up `ctx.locals[2]` (not found) and silently skipped. Fix: position-
    // based cap-arg lookup resolves the current-scope id from the `New` args.
    //
    // Two variants: post-super mutation (Bug 1a) and module-level inlining
    // (Bug 1b, exercised by calling check() at module level below).
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function check(construct: any, constructArgs: any[]) {
  let called = 0;
  class MySubclass extends construct {
    constructor() {
      super(...constructArgs);
      ++called;  // mutates AFTER super() — exercises Bug 1a stash placement
    }
  }
  new MySubclass();
  return called;
}

// Called at module level → inlined into module-init (Bug 1b: alpha-renamed ids)
console.log("duration called:", check(Temporal.Duration, [0, 0, 0, -4]));   // 1
console.log("plain-date called:", check(Temporal.PlainDate, [2021, 7, 20])); // 1
"#,
    );
    assert_eq!(
        stdout,
        "duration called: 1\n\
         plain-date called: 1\n"
    );
}

#[test]
fn temporal_plain_date_no_ctor_subclass_cell_stashed() {
    // Regression for #5587: a subclass with NO explicit constructor that
    // overrides getters must still have the Temporal cell stashed so that
    // `compare()` / `instanceof` / inherited methods use internal slots, not
    // the (potentially throwing) getter overrides.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
class AvoidGettersDate extends Temporal.PlainDate {
  // Throw so that any call to this getter causes an observable failure —
  // compare() must use internal slots and never invoke this accessor.
  get year() { throw new Error("year accessor must not be called by compare()"); }
}

const a = new AvoidGettersDate(2000, 5, 2);
const b = new Temporal.PlainDate(2006, 3, 25);
console.log("instanceof:", a instanceof Temporal.PlainDate);  // true
// compare() must use internal slots, NOT the overridden .year getter
const cmp = Temporal.PlainDate.compare(a, b);
console.log("compare:", cmp);  // -1 (2000 < 2006)
"#,
    );
    assert_eq!(
        stdout,
        "instanceof: true\n\
         compare: -1\n"
    );
}

#[test]
fn duration_add_f64_representable_precision() {
    // Regression for #5587 / test262 float64-representable-integer:
    // After `add()`, `subtract()`, and `round()`, Duration fields must be
    // clamped to their nearest float64-representable value — they must not
    // carry sub-float64 precision that JS Numbers can't express.
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
// 9007199254740991 + 9007199254740990 = 18014398509481981 (not f64-exact)
// ℝ(𝔽(18014398509481981)) = 18014398509481980
const d = new Temporal.Duration(0, 0, 0, 0, 0, 0, 0, 0, Number.MAX_SAFE_INTEGER, 0);
const result = d.add({ microseconds: Number.MAX_SAFE_INTEGER - 1 });

console.log("microseconds:", result.microseconds);  // 18014398509481980
console.log("toString:", result.toString());        // PT18014398509.48198S
// subsequent add of 1 µs must still compare equal (internal value = 18014398509481980)
console.log("compare:", Temporal.Duration.compare(result.add({ microseconds: 1 }), result));  // 0
"#,
    );
    assert_eq!(
        stdout,
        "microseconds: 18014398509481980\n\
         toString: PT18014398509.48198S\n\
         compare: 0\n"
    );
}
