//! A guard narrows a local by presence (`gen::narrow`, "Guards"): code a
//! truthy or non-nullish test of the local selects reads its values but
//! `null`/`undefined`. `mk` may fall off its end, so `a` is an Array or
//! `undefined`; a `length` read of it is an own Array length only where a
//! guard removed the `undefined`. The rule is the `if`'s, through `&&`,
//! `||`, `?:`, `!`, a predicate and an early exit alike, and in a closure
//! for a const it captures.
use super::*;

const SRC: &str = r#"
function mk(n: number): number[] | undefined { if (n > 1) return [n, 2]; }
function has(x: any): boolean { return x != null; }
function main(n: number) {
  const a = mk(n);
  console.log(a && a.length); // AND
  console.log(a ? a.length : 0); // COND
  if (a) { console.log(a.length); } // IF
  console.log(!a || a.length); // NOTOR
  console.log(a == null ? 0 : a.length); // LOOSE
  console.log(a !== undefined && a.length); // STRICT
  if (has(a)) { console.log(a.length); } // PRED
  console.log(a || a.length); // FALSY
  console.log(a === null ? 0 : a.length); // NULLONLY
  let b = mk(n);
  if (b) { b = mk(n - 5); console.log(b.length); } // WRITTEN
  let c = mk(n);
  const drop = () => { c = undefined; };
  if (c) { drop(); console.log(c.length); } // CAPTURED
}
function early(n: number) {
  const a = mk(n);
  if (!a) return 0;
  return a.length; // EARLY
}
function configure(n: number) {
  const arr = mk(n);
  function inner() { return arr ? arr.length : 0; } // CAPCONST
  const arrow = () => arr && arr.length; // CAPARROW
  let m = mk(n);
  const later = () => m ? m.length : 0; // CAPLET
  m = mk(n + 1);
  return inner() + Number(arrow()) + later();
}
function bare(n: number) {
  const a = mk(n);
  return a.length; // BARE
}
main(process.argv.length);
early(process.argv.length);
configure(process.argv.length);
bare(process.argv.length);
"#;

fn at<'a>(pf: &'a ProgramFacts, marker: &str) -> Vec<&'a SiteFacts> {
    let m = SRC
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = SRC[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "length" && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `length` site on the {marker} line");
    s
}

fn proven(pf: &ProgramFacts, marker: &str) -> bool {
    at(pf, marker)
        .iter()
        .all(|s| !matches!(s.verdict, Verdict::No(_)))
}

fn why(pf: &ProgramFacts, marker: &str) -> String {
    format!(
        "{marker}: {:?}",
        at(pf, marker)
            .iter()
            .map(|s| (&s.receivers, &s.verdict))
            .collect::<Vec<_>>()
    )
}

const GUARDED: [&str; 10] = [
    "// AND",
    "// COND",
    "// IF",
    "// NOTOR",
    "// LOOSE",
    "// STRICT",
    "// PRED",
    "// EARLY",
    "// CAPCONST",
    "// CAPARROW",
];

#[test]
fn a_presence_guard_removes_undefined_from_the_read() {
    let pf = facts(SRC);
    for m in GUARDED {
        assert!(proven(&pf, m), "{}", why(&pf, m));
    }
}

/// The controls: no guard, the falsy outcome, a guard that leaves
/// `undefined` (`=== null`), a write in the guarded code, a closure that
/// may write the local, and a closure's guard of a captured `let` its
/// creator writes (only a captured const is narrowed).
#[test]
fn a_read_no_presence_guard_covers_keeps_undefined() {
    let pf = facts(SRC);
    for m in [
        "// BARE",
        "// FALSY",
        "// NULLONLY",
        "// WRITTEN",
        "// CAPTURED",
        "// CAPLET",
    ] {
        assert!(!proven(&pf, m), "{}", why(&pf, m));
    }
}

/// `PERRY_FACTS_GUARD_PRESENCE=0`: no guard narrows by presence.
#[test]
fn without_guard_presence_no_guard_removes_undefined() {
    let pf = facts_with(
        SRC,
        FactsConfig {
            no_guard_presence: true,
            ..Default::default()
        },
    );
    for m in GUARDED {
        assert!(!proven(&pf, m), "{}", why(&pf, m));
    }
}
