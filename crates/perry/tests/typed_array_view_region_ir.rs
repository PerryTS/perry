//! Decision 69: a loop over a typed array with a COMPUTED length proves its
//! indices against the length once, in a loop region's guard, instead of
//! comparing every access against the length.
//!
//! A module-level `new Float64Array(rows.length * 7)` passed to a function
//! gets a `$spec_ta7_*` clone whose parameter is a proven view with the
//! length in the header. Before the region, each element access was the
//! checked tier: `idx <u len`, a load arm and an `undefined` arm
//! (`pview.get.*` / `pview.set.*` blocks), 1.78G instructions for n-body
//! against 0.59G with a literal length.
//!
//! With the region, each loop (and each straight-line run left by an
//! unrolled loop) is versioned once: the guard reads the length with a plain
//! load and compares the largest index end against it; the copy that runs
//! when the guard passes has NO checked access. The copy that runs when it
//! fails is today's code, so the clone keeps exactly the checked accesses it
//! had without the region (`PERRY_REGION_VIEWS=0`), and no more: a checked
//! access inside a guarded copy would double that count.
//!
//! The behavioural half (detach and resize mid-loop, reassignment, aliasing,
//! out-of-range indices, every typed-array kind) is
//! `test-files/test_gap_typed_array_view_region.ts`, compared against node.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const NBODY: &str = r#"
const ROWS: number[][] = [[1, 2], [3, 4], [5, 6], [7, 8], [9, 10]];
function advance(a: Float64Array, dt: number): void {
  for (let i = 0; i < 5; i++) {
    const oi = i * 7;
    for (let j = i + 1; j < 5; j++) {
      const oj = j * 7;
      const dx = a[oi] - a[oj], dy = a[oi + 1] - a[oj + 1];
      const d2 = dx * dx + dy * dy + 1;
      const mag = dt / (d2 * Math.sqrt(d2));
      const bim = a[oi + 6] * mag, bjm = a[oj + 6] * mag;
      a[oi + 3] -= dx * bjm;
      a[oj + 3] += dx * bim;
    }
  }
  for (let i = 0; i < 5; i++) { const o = i * 7; a[o] += dt * a[o + 3]; }
}
const a = new Float64Array(ROWS.length * 7);
for (let i = 0; i < 35; i++) a[i] = i + 1;
for (let i = 0; i < 100; i++) advance(a, 0.01);
console.log(a[3].toFixed(6), a[10].toFixed(6));
"#;

const BUMP: &str = r#"
const X: number[] = [1, 2, 3, 4, 5, 6];
function bump(a: Int32Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    s += a[i] * 2 + a[i];
  }
  return s;
}
const t = new Int32Array(X.length * 3);
for (let i = 0; i < t.length; i++) t[i] = i;
let r = 0;
for (let k = 0; k < 1000; k++) r += bump(t, t.length);
console.log(r);
"#;

/// The body of the first `define` whose symbol starts with `prefix`.
fn function_body<'a>(ir: &'a str, prefix: &str) -> Option<(&'a str, &'a str)> {
    let needle = format!("@{prefix}");
    let mut offset = 0;
    for line in ir.split_inclusive('\n') {
        if line.starts_with("define ") {
            if let Some(at) = line.find(&needle) {
                let open = line[at..].find('(')? + at;
                let symbol = &line[at + 1..open];
                let rest = &ir[offset..];
                let end = rest.find("\n}\n").map(|i| i + 3).unwrap_or(rest.len());
                return Some((symbol, &rest[..end]));
            }
        }
        offset += line.len();
    }
    None
}

fn compile_ir(name: &str, source: &str, views: bool) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join(format!("{name}.ts"));
    std::fs::write(&entry, source).expect("write entry");
    let ll = dir.path().join("ll");
    std::fs::create_dir_all(&ll).expect("ll dir");
    let mut cmd = Command::new(perry_bin());
    cmd.current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join(name))
        .arg("--no-cache")
        .arg("--no-link")
        .env("PERRY_SAVE_LL", &ll);
    if !views {
        cmd.env("PERRY_REGION_VIEWS", "0");
    }
    let out = cmd.output().expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    std::fs::read_dir(&ll)
        .expect("read ll dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read .ll"))
        .collect()
}

/// Checked element accesses: one `pview.get.oob` block per checked read and
/// one `pview.set.done` block per checked store.
fn checked_accesses(body: &str) -> usize {
    body.lines()
        .filter(|l| l.starts_with("pview.get.oob.") || l.starts_with("pview.set.done."))
        .count()
}

fn count(body: &str, needle: &str) -> usize {
    body.matches(needle).count()
}

/// The guarded copies carry no checked access: the clone has exactly the
/// checked accesses of its unguarded copies, which are today's code.
fn assert_guarded_copies_unchecked(name: &str, source: &str, clone: &str, elem_load: &str) {
    let on = compile_ir(name, source, true);
    let off = compile_ir(name, source, false);
    let (symbol, with) =
        function_body(&on, clone).unwrap_or_else(|| panic!("no {clone} clone in:\n{on}"));
    let (_, without) =
        function_body(&off, clone).unwrap_or_else(|| panic!("no {clone} clone (views off)"));
    let checked_off = checked_accesses(without);
    // The witness: without the region the clone's accesses are checked, so
    // the assertions below are not vacuous.
    assert!(
        checked_off > 0,
        "{symbol}: no checked access even without the region:\n{without}"
    );
    // A region formed: the loop is versioned and the guard compares against
    // the length.
    assert!(
        count(with, "rloop.fast") > 0,
        "{symbol}: no region formed:\n{with}"
    );
    // The guarded copy accesses elements (more element loads than the
    // unguarded code alone)...
    assert!(
        count(with, elem_load) > count(without, elem_load),
        "{symbol}: the guarded copy has no element loads:\n{with}"
    );
    // ...and none of them is checked: every checked access is the plain copy's.
    assert_eq!(
        checked_accesses(with),
        checked_off,
        "{symbol}: a guarded copy kept a per-access bounds check:\n{with}"
    );
    // No access in a guarded copy asserts its bounds with an assume (the
    // guard proved them against a plain length load).
    assert!(
        count(with, "@llvm.assume") <= count(without, "@llvm.assume"),
        "{symbol}: the region added assumes:\n{with}"
    );
}

#[test]
fn computed_length_nbody_guarded_copies_have_no_bounds_checks() {
    assert_guarded_copies_unchecked(
        "nbody_view_region",
        NBODY,
        "perry_fn_nbody_view_region_ts__advance$spec_ta7_",
        "load double, ptr",
    );
    // The constant end of every index (`oj + 6` with `j < 5`: 35) is
    // compared once against the header length.
    let on = compile_ir("nbody_view_region", NBODY, true);
    let (symbol, with) =
        function_body(&on, "perry_fn_nbody_view_region_ts__advance$spec_ta7_").expect("clone");
    assert!(
        with.contains("icmp ule i32 35, "),
        "{symbol}: no `35 <= length` guard:\n{with}"
    );
}

#[test]
fn symbolic_bound_int32_loop_guarded_copy_has_no_bounds_checks() {
    assert_guarded_copies_unchecked(
        "bump_view_region",
        BUMP,
        "perry_fn_bump_view_region_ts__bump$spec_ta4",
        "load i32, ptr",
    );
}

#[test]
fn own_and_other_length_bounds_have_unchecked_guarded_copies() {
    for (name, bound) in [
        ("own_length", "a.length"),
        ("other_length", "b.length"),
        ("length_minus_two", "a.length - 2"),
        ("length_minus_k", "a.length - k"),
        ("length_inclusive", "a.length - 1"),
    ] {
        let cmp = if name == "length_inclusive" {
            "<="
        } else {
            "<"
        };
        let source = format!(
            r#"
const X: number[] = [1, 2, 3, 4, 5, 6];
function bump(a: Int32Array, b: Int32Array): number {{
  const k = 2;
  let s = 0;
  for (let i = 0; i {cmp} {bound}; i++) s += a[i] * 2 + a[i];
  return s;
}}
const a = new Int32Array(X.length * 3);
const b = new Int32Array(X.length * 2);
for (let i = 0; i < a.length; i++) a[i] = i;
console.log(bump(a, b));
"#
        );
        let prefix = format!("perry_fn_{name}_ts__bump$spec_ta4");
        let on = compile_ir(name, &source, true);
        let off = compile_ir(name, &source, false);
        let (_, with) = function_body(&on, &prefix).expect("typed-array clone");
        let (_, without) = function_body(&off, &prefix).expect("typed-array clone without regions");
        assert!(
            without.contains("icmp ult i32"),
            "no per-access witness: {without}"
        );
        assert!(with.contains("rloop.fast"), "no guarded copy: {with}");
        let label = with
            .lines()
            .find(|l| l.starts_with("rloop.fast.") && l.ends_with(':'))
            .unwrap();
        let fast = &with[with.find(label).unwrap() + label.len()..];
        let fast = fast.split("\n\n").next().unwrap();
        assert!(fast.matches("load i32, ptr").count() >= 2, "{fast}");
        assert!(
            !fast.contains("icmp ult")
                && !fast.contains("@llvm.assume")
                && !fast.contains("pview.get.oob"),
            "guarded access is checked: {fast}"
        );
        assert_eq!(checked_accesses(with), checked_accesses(without), "{with}");
    }
}
