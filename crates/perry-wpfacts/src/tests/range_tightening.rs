//! R1 (range tightening): a rule's result is not widened past the rule over
//! its operands (`solve::grow_rule_range`), and a counter write in a branch
//! that always leaves does not end the narrowed reads after it
//! (`gen::narrow`, "Writes on a leaving path"). Each verdict is read where it
//! is consumed: the key interval of a field read call (`at(k)` over a board
//! born dense with 128 elements) and whether it is in the floor.

use super::*;

/// `(call text, key interval, in floor)` of every field read call of `src`.
fn keys(src: &str, cfg: FactsConfig) -> Vec<(String, Option<(i64, i64)>, bool)> {
    let pf = facts_with(src, cfg);
    let mut out = Vec::new();
    for mb in &pf.bindings {
        for (offset, c) in &mb.field_read_calls {
            // Offsets are 1-based.
            let rest = &src[*offset as usize - 1..];
            let end = rest.find(';').unwrap_or(rest.len());
            let key = mb
                .call_arg_ranges
                .binary_search_by_key(offset, |(o, _)| *o)
                .ok()
                .and_then(|at| mb.call_arg_ranges[at].1.get(c.index_arg as usize).copied())
                .flatten();
            out.push((rest[..end].to_string(), key, c.in_floor));
        }
    }
    out
}

fn call(calls: &[(String, Option<(i64, i64)>, bool)], text: &str) -> (Option<(i64, i64)>, bool) {
    let c = calls
        .iter()
        .find(|(t, ..)| t.starts_with(text))
        .unwrap_or_else(|| panic!("no call {text}: {calls:?}"));
    (c.1, c.2)
}

const BOARD: &str = "class B {\n\
  board: number[] = new Array(128).fill(0);\n\
  at(i: number): number { return this.board[i] ?? 0; }\n";

fn program(methods: &str, calls: &str) -> String {
    format!("{BOARD}{methods}\n}}\nconst b = new B();\nconsole.log({calls});\n")
}

/// The shapes of the C3 probe (qb9 c3/probe/rg): one method per key.
fn probe() -> String {
    program(
        "  mulOnly(): number { let s = 0; for (let r = 0; r < 8; r++) s += this.at(r * 16); return s; }\n\
           mulAdd(): number { let s = 0; for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r * 16 + f); return s; }\n\
           addOnly(): number { let s = 0; for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r + f); return s; }\n\
           down(): number { let s = 0; for (let r = 7; r >= 0; r--) for (let f = 0; f < 8; f++) s += this.at(r * 16 - f + 7); return s; }\n\
           skip(): number { let s = 0; for (let q = 0; q < 128; q++) { if ((q & 0x88) !== 0) { q += 7; continue; } s += this.at(q + 0); } return s; }\n\
           plain(): number { let s = 0; for (let q = 0; q < 128; q++) { if ((q & 0x88) !== 0) continue; s += this.at(q - 0); } return s; }\n\
           elseLeaves(): number { let s = 0; for (let q = 0; q < 128; q++) { if ((q & 0x88) === 0) { s += 1; } else { q += 7; continue; } s += this.at(q * 1); } return s; }",
        "b.mulOnly(), b.mulAdd(), b.addOnly(), b.down(), b.skip(), b.plain(), b.elseLeaves()",
    )
}

#[test]
fn a_sum_of_two_counters_is_their_hull() {
    let calls = keys(&probe(), FactsConfig::default());
    assert_eq!(call(&calls, "this.at(r * 16)"), (Some((0, 112)), true));
    assert_eq!(call(&calls, "this.at(r * 16 + f)"), (Some((0, 119)), true));
    assert_eq!(call(&calls, "this.at(r + f)"), (Some((0, 14)), true));
    assert_eq!(
        call(&calls, "this.at(r * 16 - f + 7)"),
        (Some((0, 119)), true)
    );
}

#[test]
fn a_write_in_a_branch_that_leaves_keeps_the_narrowed_reads() {
    let calls = keys(&probe(), FactsConfig::default());
    assert_eq!(call(&calls, "this.at(q + 0)"), (Some((0, 127)), true));
    assert_eq!(call(&calls, "this.at(q - 0)"), (Some((0, 127)), true));
    assert_eq!(call(&calls, "this.at(q * 1)"), (Some((0, 127)), true));
}

#[test]
fn the_switches_off_give_the_widened_intervals_back() {
    let rules_off = keys(
        &probe(),
        FactsConfig {
            no_exact_rules: true,
            ..Default::default()
        },
    );
    let (k, _) = call(&rules_off, "this.at(r + f)");
    assert!(k.is_some_and(|(_, hi)| hi > 14), "{k:?}");
    let (k, in_floor) = call(&rules_off, "this.at(r * 16 + f)");
    assert!(k.is_some_and(|(_, hi)| hi > 127) && !in_floor, "{k:?}");
    let writes_off = keys(
        &probe(),
        FactsConfig {
            no_leaving_writes: true,
            ..Default::default()
        },
    );
    let (k, in_floor) = call(&writes_off, "this.at(q + 0)");
    assert!(k.is_some_and(|(_, hi)| hi > 127) && !in_floor, "{k:?}");
    // A bare `continue` writes nothing: narrowed either way.
    assert_eq!(call(&writes_off, "this.at(q - 0)"), (Some((0, 127)), true));
}

/// Negative controls: a write that reaches a later read keeps it wide.
fn reaching() -> String {
    program(
        "  falls(): number { let s = 0; for (let q = 0; q < 128; q++) { if ((q & 0x88) !== 0) { q += 7; } s += this.at(q + 0); } return s; }\n\
           inSwitch(k: number): number { let s = 0; for (let q = 0; q < 128; q++) { switch (k) { case 1: if (q > 3) { q += 200; break; } s += 1; } s += this.at(q - 0); } return s; }\n\
           inTry(): number { let s = 0; for (let q = 0; q < 128; q++) { try { if (q > 3) { q += 200; continue; } } finally { s += this.at(q * 1); } } return s; }\n\
           labeled(): number { let s = 0; outer: for (let q = 0; q < 128; q++) { if ((q & 0x88) !== 0) { q += 200; continue outer; } s += this.at(q | 0); } return s; }\n\
           sumPast(): number { let s = 0; for (let r = 0; r < 8; r++) for (let f = 0; f < 8; f++) s += this.at(r * 16 + f + 9); return s; }",
        "b.falls(), b.inSwitch(1), b.inSwitch(2), b.inTry(), b.labeled(), b.sumPast()",
    )
}

#[test]
fn a_write_that_reaches_a_later_read_keeps_it_wide() {
    let calls = keys(&reaching(), FactsConfig::default());
    for (text, reach) in [
        // `q + 7` falls through to the read.
        ("this.at(q + 0)", 134),
        // The `break` lands after the `switch`, before the read.
        ("this.at(q - 0)", 327),
        // The `finally` runs after the `continue`.
        ("this.at(q * 1)", 327),
        // A labeled jump is not followed (conservative).
        ("this.at(q | 0)", 327),
        // The sum's hull is past the floor.
        ("this.at(r * 16 + f + 9)", 128),
    ] {
        let (k, in_floor) = call(&calls, text);
        assert!(
            k.is_none_or(|(_, hi)| hi >= reach) && !in_floor,
            "{text}: {k:?} {in_floor}"
        );
    }
}

#[test]
fn the_sabotage_arms_claim_a_read_in_the_floor_that_is_not() {
    let leaving = keys(
        &reaching(),
        FactsConfig {
            sabotage: Some("leavingwrite".into()),
            ..Default::default()
        },
    );
    assert!(call(&leaving, "this.at(q + 0)").1, "{leaving:?}");
    let exact = keys(
        &reaching(),
        FactsConfig {
            sabotage: Some("exactrule".into()),
            ..Default::default()
        },
    );
    assert!(call(&exact, "this.at(r * 16 + f + 9)").1, "{exact:?}");
}
