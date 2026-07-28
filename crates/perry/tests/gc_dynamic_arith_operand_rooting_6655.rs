//! Regression tests for #6655 — raw NaN-boxed operands held across GC-capable
//! coercions in the dynamic operator helpers.
//!
//! `to_numeric` / `rel_to_primitive` / `js_jsvalue_to_string` on an object
//! operand run a user `Symbol.toPrimitive` / `valueOf` / `toString`, which can
//! allocate, trigger a GC and *evacuate* (move) live objects. Pre-fix the
//! operator helpers held the other operand as a raw `f64` in a Rust local:
//!
//! ```ignore
//! let a = to_numeric(a);   // user valueOf -> allocate -> GC -> evacuation
//! let b = to_numeric(b);   // raw `b` was held UNROOTED across the line above
//! ```
//!
//! A Rust local is neither a GC root nor a shadow slot, so after the first
//! coercion `b` could name a forwarded address. The same shape hit the coerced
//! `a` when it is a BigInt pointer and the *second* operand is the allocating
//! one — hence every operator below is exercised in BOTH operand orders.
//!
//! The programs run with `PERRY_GC_FORCE_EVACUATE=1` (stress-copies every
//! marked non-pinned nursery object, so a stale pointer becomes a deterministic
//! wrong answer / crash instead of a rare one) and `PERRY_GC_VERIFY_EVACUATION=1`
//! (panics if any mutable live slot still points at a forwarded object).
//!
//! Coverage: `js_dynamic_{mul,sub,div,mod,pow,shr,shl,bitand,bitor,bitxor,ushr}`,
//! `js_numeric_step`, `abstract_relational` behind `js_rel_{lt,gt,le,ge}`, and
//! `js_string_concat_value` / `js_value_concat_string`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run_forced_evacuation(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .env("PERRY_GC_FORCE_EVACUATE", "1")
        .env("PERRY_GC_VERIFY_EVACUATION", "1")
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed under forced evacuation (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Shared prelude: an operand whose `valueOf` churns the nursery and forces an
/// explicit collection, and a plain movable object operand that (pre-fix) was
/// held unrooted across the other operand's coercion.
const PRELUDE: &str = r#"
// Allocate a lot of short-lived nursery objects, then force a collection so a
// GC + evacuation happens *inside* the operand coercion rather than by luck.
function churnAndCollect(): void {
  let sink = 0;
  for (let i = 0; i < 3000; i++) {
    const tmp = { i, s: "pad" + i };
    sink += tmp.s.length > 0 ? 1 : 0;
  }
  if (sink !== 3000) throw new Error("churn miscounted");
  (globalThis as any).gc?.();
}

// GC-capable operand: its valueOf runs user JS that allocates and collects.
function heavy(v: any): any {
  return {
    valueOf() {
      churnAndCollect();
      return v;
    },
  };
}

// Plain heap-object operand. It is a movable pointer, so pre-fix it went stale
// whenever it sat on the far side of the other operand's coercion.
function plain(v: any): any {
  return {
    valueOf() {
      return v;
    },
  };
}

let failures = 0;
function check(name: string, got: any, want: any): void {
  if (got !== want) {
    failures++;
    console.log("FAIL " + name + " got=" + String(got) + " want=" + String(want));
  }
}
"#;

/// Every `to_numeric(a); to_numeric(b)` operator, in both operand orders.
///
/// `heavy-first` exercises the primary hazard (raw second operand held across
/// the first coercion). `heavy-second` exercises the coerced-`a` hazard, which
/// is the one that bites when `a` resolves to a BigInt pointer.
#[test]
fn dynamic_arith_operands_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// --- heavy operand FIRST: raw `b` held across to_numeric(a) ---
check("mul", heavy(12) * plain(3), 36);
check("sub", heavy(12) - plain(3), 9);
check("div", heavy(12) / plain(3), 4);
check("mod", heavy(13) % plain(5), 3);
check("pow", heavy(2) ** plain(10), 1024);
check("shr", heavy(1024) >> plain(3), 128);
check("shl", heavy(3) << plain(4), 48);
check("bitand", heavy(12) & plain(10), 8);
check("bitor", heavy(12) | plain(10), 14);
check("bitxor", heavy(12) ^ plain(10), 6);
check("ushr", heavy(-16) >>> plain(1), 2147483640);

// --- heavy operand SECOND: coerced `a` held across to_numeric(b) ---
check("mul-rev", plain(12) * heavy(3), 36);
check("sub-rev", plain(12) - heavy(3), 9);
check("div-rev", plain(12) / heavy(3), 4);
check("mod-rev", plain(13) % heavy(5), 3);
check("pow-rev", plain(2) ** heavy(10), 1024);
check("shr-rev", plain(1024) >> heavy(3), 128);
check("shl-rev", plain(3) << heavy(4), 48);
check("bitand-rev", plain(12) & heavy(10), 8);
check("bitor-rev", plain(12) | heavy(10), 14);
check("bitxor-rev", plain(12) ^ heavy(10), 6);
check("ushr-rev", plain(-16) >>> heavy(1), 2147483640);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "dynamic arithmetic produced a wrong result under forced evacuation"
    );
}

/// BigInt operands are heap pointers, so they are the sharpest probe for the
/// "coerced `a` goes stale across `to_numeric(b)`" half of the bug: `a`
/// resolves to a BigInt pointer and then the heavy second operand collects.
/// Also covers `js_numeric_step` (`++`/`--`), which allocates `1n` while the
/// incoming BigInt operand is live.
#[test]
fn dynamic_bigint_operands_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// `a` coerces to a BigInt pointer, then the heavy `b` collects underneath it.
check("big-mul-rev", plain(11n) * heavy(3n), 33n);
check("big-sub-rev", plain(20n) - heavy(3n), 17n);
check("big-div-rev", plain(36n) / heavy(3n), 12n);
check("big-mod-rev", plain(13n) % heavy(5n), 3n);
check("big-pow-rev", plain(2n) ** heavy(10n), 1024n);
check("big-and-rev", plain(12n) & heavy(10n), 8n);
check("big-or-rev", plain(12n) | heavy(10n), 14n);
check("big-xor-rev", plain(12n) ^ heavy(10n), 6n);
check("big-shl-rev", plain(3n) << heavy(4n), 48n);
check("big-shr-rev", plain(1024n) >> heavy(3n), 128n);

// heavy FIRST, raw BigInt `b` held across the coercion.
check("big-mul", heavy(11n) * plain(3n), 33n);
check("big-add", heavy(11n) + plain(3n), 14n);

// js_numeric_step: `1n` is allocated while the operand BigInt is live.
let step: any = 9007199254740993n;
for (let i = 0; i < 200; i++) {
  churnAndCollect();
  step++;
}
check("bigint-step", step, 9007199254741193n);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "dynamic BigInt arithmetic produced a wrong result under forced evacuation"
    );
}

/// `abstract_relational` (behind `js_rel_lt/gt/le/ge`) has the identical
/// `rel_to_primitive(x); rel_to_primitive(y)` prelude, and additionally held
/// `px` — often a freshly allocated heap string from the `DefaultString` arm —
/// across the second coercion.
#[test]
fn relational_operands_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
check("lt", heavy(3) < plain(10), true);
check("gt", heavy(30) > plain(10), true);
check("le", heavy(10) <= plain(10), true);
check("ge", heavy(10) >= plain(10), true);

check("lt-rev", plain(3) < heavy(10), true);
check("gt-rev", plain(30) > heavy(10), true);
check("le-rev", plain(10) <= heavy(10), true);
check("ge-rev", plain(10) >= heavy(10), true);

// String operands take the lexicographic arm, where the ToPrimitive result is
// itself a fresh heap string that must survive the other side's coercion.
check("lt-str", heavy("apple") < plain("banana"), true);
check("gt-str", plain("cherry") > heavy("banana"), true);

// Mixed BigInt/string and BigInt/number arms re-derive the BigInt pointer
// after an allocating StringToBigInt / ToNumber step.
check("big-lt-str", heavy(5n) < plain("10"), true);
check("big-gt-num", plain(10n) > heavy(5), true);

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "relational comparison produced a wrong result under forced evacuation"
    );
}

/// `js_string_concat_value` / `js_value_concat_string` hold the raw
/// `*const StringHeader` operand across `string_storage_alloc` (fast path) and
/// across `js_jsvalue_to_string`'s user `toString` (slow path).
#[test]
fn string_concat_operands_survive_forced_evacuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = format!(
        "{PRELUDE}{}",
        r#"
// Build the fixed operand dynamically so it is a movable nursery string
// rather than a pinned static literal.
const prefix: string = "pre-" + String(7);
const suffix: string = "-post" + String(9);

check("concat-value", prefix + heavy(5), "pre-75");
check("value-concat", heavy(5) + suffix, "5-post9");

// Repeat under sustained churn so the allocation inside the concat itself has
// a live nursery to evacuate.
for (let i = 0; i < 50; i++) {
  const p: string = "p" + String(i);
  check("concat-loop", p + heavy(i), "p" + String(i) + String(i));
}

console.log("failures:", failures);
"#
    );
    let stdout = compile_and_run_forced_evacuation(dir.path(), &source);
    assert_eq!(
        stdout, "failures: 0\n",
        "string concatenation produced a wrong result under forced evacuation"
    );
}
