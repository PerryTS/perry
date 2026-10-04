//! #11810: an alias of a proven typed-array view keeps the view.
//!
//! A module-level `new Float64Array(35)` passed to a function gets a
//! `$spec_ta7x35_*` clone whose parameter is a proven view (fixed kind, data
//! pointer and length). `a[oi + 3] -= v` lowers through a compiler temp,
//! `let __cmpd_base = a`, and binding that alias used to demote BOTH names to
//! `MayAlias`. A demoted view is refused by the proven tiers and, because a
//! tracked view "owns" its receiver, also by the per-site guarded reads, so
//! every later element read in the clone became a `js_typed_array_get` call
//! plus a `js_number_coerce`: 14.09G instructions against 5.62G for the same
//! program with a computed length, which gets no clone at all.
//!
//! This pins the clone's element accesses: no typed-array runtime helper and
//! no coercion call is left in it. The behavioural half (the same kernel plus
//! alias, reassignment and `.buffer` cases) is
//! `test-files/test_gap_11810_typed_array_alias_view.ts`, compared against
//! node.
//!
//! The computed-length half: `new Float64Array(rows.length * 7)` is the same
//! length form (a product is never an Object), so it gets the same clone,
//! `$spec_ta7_*` with the length read from the header instead of a constant,
//! and its element accesses and arithmetic must be just as native. Its
//! behavioural half is `test-files/test_gap_11810_computed_length_view.ts`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
function advance(a: Float64Array, dt: number): void {
  for (let i = 0; i < 5; i++) {
    const oi = i * 7;
    for (let j = i + 1; j < 5; j++) {
      const oj = j * 7;
      const dx = a[oi] - a[oj];
      const bim = a[oi + 6] * dt, bjm = a[oj + 6] * dt;
      a[oi + 3] -= dx * bjm;
      a[oj + 3] += dx * bim;
    }
  }
  for (let i = 0; i < 5; i++) { const o = i * 7; a[o] += dt * a[o + 3]; }
}
const a = new Float64Array(35);
for (let i = 0; i < 35; i++) a[i] = i + 1;
for (let i = 0; i < 100; i++) advance(a, 0.01);
console.log(a[3].toFixed(6), a[10].toFixed(6));
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

#[test]
fn literal_length_spec_clone_element_access_has_no_runtime_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("nbody11810.ts");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let ll = dir.path().join("ll");
    std::fs::create_dir_all(&ll).expect("ll dir");
    let out = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join("nbody11810"))
        .arg("--no-cache")
        .arg("--no-link")
        .env("PERRY_SAVE_LL", &ll)
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let ir: String = std::fs::read_dir(&ll)
        .expect("read ll dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read .ll"))
        .collect();

    // The witness that this is the case the issue is about: the literal
    // length produced the specialized typed-array clone. Without it the
    // assertions below would be vacuous.
    let (symbol, clone) = function_body(&ir, "perry_fn_nbody11810_ts__advance$spec_ta7x35")
        .unwrap_or_else(|| panic!("no $spec_ta7x35 clone of advance in:\n{ir}"));

    // The clone does read and write elements natively.
    assert!(
        clone.contains("load double, ptr") && clone.contains("store double"),
        "{symbol}: no native element access:\n{clone}"
    );
    for helper in [
        "@js_typed_array_get(",
        "@js_typed_array_read_f64(",
        "@js_number_coerce(",
        "@js_typed_array_index_get_dynamic(",
        "@lookup_registered_typed_array_kind(",
    ] {
        assert!(
            !clone.contains(helper),
            "{symbol}: element access fell back to `{helper}`:\n{clone}"
        );
    }
}

const COMPUTED_SOURCE: &str = r#"
const rows: number[][] = [[1, 2, 3, 4, 5, 6, 7], [8, 9, 10, 11, 12, 13, 14],
  [15, 16, 17, 18, 19, 20, 21], [22, 23, 24, 25, 26, 27, 28], [29, 30, 31, 32, 33, 34, 35]];
function advance(a: Float64Array, dt: number): void {
  for (let i = 0; i < 5; i++) {
    const oi = i * 7;
    for (let j = i + 1; j < 5; j++) {
      const oj = j * 7;
      const dx = a[oi] - a[oj], dy = a[oi + 1] - a[oj + 1];
      const d2 = dx * dx + dy * dy;
      const mag = dt / (d2 * Math.sqrt(d2));
      const bim = a[oi + 6] * mag, bjm = a[oj + 6] * mag;
      a[oi + 3] -= dx * bjm;
      a[oj + 3] += dx * bim;
    }
  }
  for (let i = 0; i < 5; i++) { const o = i * 7; a[o] += dt * a[o + 3]; }
}
const a = new Float64Array(rows.length * 7);
for (let i = 0; i < 5; i++) for (let k = 0; k < 7; k++) a[i * 7 + k] = rows[i][k];
for (let i = 0; i < 100; i++) advance(a, 0.01);
console.log(a[3].toFixed(6), a[10].toFixed(6));
"#;

#[test]
fn computed_length_spec_clone_element_access_and_arithmetic_have_no_runtime_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("nbodycomp.ts");
    std::fs::write(&entry, COMPUTED_SOURCE).expect("write entry");
    let ll = dir.path().join("ll");
    std::fs::create_dir_all(&ll).expect("ll dir");
    let out = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(dir.path().join("nbodycomp"))
        .arg("--no-cache")
        .arg("--no-link")
        .env("PERRY_SAVE_LL", &ll)
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let ir: String = std::fs::read_dir(&ll)
        .expect("read ll dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "ll"))
        .map(|e| std::fs::read_to_string(e.path()).expect("read .ll"))
        .collect();

    // The witness: the computed length produced a typed-array clone with NO
    // constant length (`ta7_`, not `ta7x<n>`). Without it the assertions below
    // would be vacuous.
    let (symbol, clone) = function_body(&ir, "perry_fn_nbodycomp_ts__advance$spec_ta7_")
        .unwrap_or_else(|| panic!("no $spec_ta7_ clone of advance in:\n{ir}"));

    assert!(
        clone.contains("load double, ptr") && clone.contains("store double"),
        "{symbol}: no native element access:\n{clone}"
    );
    for helper in [
        "@js_typed_array_get(",
        "@js_typed_array_read_f64(",
        "@js_number_coerce(",
        "@js_typed_array_index_get_dynamic(",
        "@lookup_registered_typed_array_kind(",
        "@js_dyn_index_set_strict(",
        "@js_dynamic_mul(",
        "@js_dynamic_sub(",
        "@js_dynamic_string_or_number_add(",
    ] {
        assert!(
            !clone.contains(helper),
            "{symbol}: element access or arithmetic fell back to `{helper}`:\n{clone}"
        );
    }
}
