//! PERFNOW34 part 1: a `for (init; c; u) B` is `init; while (c) { B; u }`,
//! so B and u see `c` true as a `while` body does (`gen::walk`,
//! `Stmt::For`; `narrow_guard`). `mk` may fall off its end, so `a` is an
//! Array or `undefined`; a `length` read of it is an own Array length only
//! where the loop test removed the `undefined`. A write in the body is seen
//! by the reads after it (`narrowed_write`), and the counter `narrow_for`
//! narrows keeps its ranges.
use super::*;

const SRC: &str = r#"
function mk(n: number): number[] | undefined { if (n > 1) return [n, 2]; }
function main(n: number) {
  for (let a = mk(n); a !== undefined; a = mk(n - 1)) {
    console.log(a.length); // BODY
    break;
  }
  for (let b = mk(n); b !== undefined; b = mk(b.length - 3)) { // UPDATE
    n -= 1;
  }
  for (let c = mk(n); c !== undefined; c = mk(n - 2)) {
    c = mk(n - 5);
    console.log(c.length); // WRITTEN
  }
  let d = mk(n);
  for (let i = 0; i < 3 && d !== undefined; i++) {
    console.log(d.length); // COUNTER
    d = mk(i);
  }
}
main(process.argv.length);
"#;

/// The receivers of the `length` reads on the line of `marker`.
fn receivers(pf: &ProgramFacts, marker: &str) -> Vec<String> {
    let m = SRC
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = SRC[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "length" && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `length` site on the {marker} line");
    s.iter().flat_map(|s| s.receivers.iter().cloned()).collect()
}

/// A receiver `undefined` (the `length` read throws) is what the test
/// removes.
fn has_undefined(r: &[String]) -> bool {
    r.iter().any(|x| x.contains("undefined"))
}

#[test]
fn for_test_narrows_body_and_update() {
    let pf = facts(SRC);
    for m in ["BODY", "UPDATE", "COUNTER"] {
        let r = receivers(&pf, m);
        assert!(!has_undefined(&r), "{m}: {r:?}");
    }
    // A write in the body reaches the read after it.
    let r = receivers(&pf, "WRITTEN");
    assert!(has_undefined(&r), "WRITTEN: {r:?}");
}
