//! −0 tolerance (RFC §10.3 as amended, PR D): a Number node keeps its
//! integer interval although it may hold `-0` exactly when no use can
//! observe the sign of a zero (`zero_sign`), and int demand flows through
//! value filters (`?? 0`).

use super::*;
use crate::FactsConfig;

/// The `# rest elem` line of the program's one board array, with the
/// tolerance on or off.
fn board(src: &str, on: bool) -> String {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            no_zero_sign: !on,
            ..Default::default()
        },
    );
    let ls: Vec<&String> = pf
        .rep_probe
        .iter()
        .filter(|l| l.starts_with("# rest elem "))
        .collect();
    ls.iter()
        .find(|l| l.contains("Array"))
        .map(|l| l.to_string())
        .unwrap_or_else(|| panic!("no board line in {ls:#?}"))
}

/// A board whose cells hold `color * kind` (`[-3, 3]`, `-0` when `kind` is
/// 0 and `color` -1), read through `?? 0` and used by `use_` (a statement
/// over `v`, the read cell).
fn program(use_: &str) -> String {
    format!(
        r#"
class B {{
  cells: number[] = new Array(8).fill(0);
  put(i: number, color: number, kind: number): void {{ this.cells[i & 7] = color * kind; }}
  get(i: number): number {{ return this.cells[i & 7] ?? 0; }}
}}
const b = new B();
for (let k = 0; k < 10; k++) b.put(k, k % 2 === 0 ? 1 : -1, k & 3);
let s = 0;
for (let k = 0; k < 8; k++) {{
  const v = b.get(k);
  if (v === 0) s++;
  s += v & 1;
  {use_}
}}
console.log(s);
"#
    )
}

#[test]
fn an_unobserved_negative_zero_keeps_the_interval_and_the_filter_passes_demand() {
    let on = board(&program(""), true);
    assert!(on.contains("range=[-3,3]"), "{on}");
    assert!(on.contains("demand=1"), "{on}");
    assert!(on.contains("zsign=0"), "{on}");
    // Absorbing uses: compares, keys, `Math.abs`, concatenation, `|`.
    for use_ in [
        "if (v > 0) s += 2;",
        "s += [1, 2, 3, 4][Math.abs(v)] ?? 0;",
        "if ('' + v === '0') s++;",
        "s += v | 0;",
        "s += v * 2 === 4 ? 1 : 0;",
    ] {
        let l = board(&program(use_), true);
        assert!(l.contains("range=[-3,3]"), "{use_}: {l}");
    }
}

#[test]
fn without_the_tolerance_a_possible_negative_zero_is_top() {
    let off = board(&program(""), false);
    assert!(off.contains("range=top"), "{off}");
}

#[test]
fn every_sign_observing_use_makes_it_top() {
    for use_ in [
        // Negative controls: each observes the sign of a zero (`String`,
        // template literals and `console.log` of a slot conservatively).
        "s += 1 / v;",
        "s += 5 / v;",
        "s += v % 2;",
        "if (Object.is(v, -0)) s++;",
        "s += Math.sign(v);",
        "s += Math.min(v, 0);",
        "s += Math.atan2(v, -1);",
        "s += v ** -1;",
        "console.log(v);",
        "s += JSON.stringify(b.cells).length;",
        "console.log(b.cells);",
        "s += String(v).length + `${v}`.length;",
        "(globalThis as any).keep = b; s += Object.is((globalThis as any).keep.get(1), -0) ? 1 : 0;",
        "s += [v].map((x) => 1 / x)[0];",
        "s += (+v === 0 && 1 / +v < 0) ? 1 : 0;",
        "s += Math.max(...b.cells);",
    ] {
        let l = board(&program(use_), true);
        assert!(l.contains("range=top"), "{use_}: {l}");
        assert!(l.contains("zsign=1") || l.contains("range=top"), "{use_}: {l}");
    }
}

#[test]
fn the_sabotage_drops_the_division_seed() {
    let pf = facts_with(
        &program("s += 1 / v;"),
        FactsConfig {
            diagnostics: true,
            sabotage: Some("zerosign".into()),
            ..Default::default()
        },
    );
    let l = pf
        .rep_probe
        .iter()
        .find(|l| l.starts_with("# rest elem ") && l.contains("Array"))
        .cloned()
        .unwrap_or_default();
    assert!(l.contains("range=[-3,3]"), "{l}");
}
