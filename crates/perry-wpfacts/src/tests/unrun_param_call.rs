//! DECISIONS 165 with 144: a parameter of a published method that no unit
//! runs stands for no value of another unit. A call through it (here
//! `moves.slice()`) is recorded for a unit that will run the method, and
//! has no result in the meantime: it is not unknown code's ⊤, whose
//! computed store would write every key of every builtin prototype and
//! poison the other units through the shared writes (which only grow).
//! Once a unit runs the method, the owner solves again with the parameter
//! described (the caller's values beside it) or bare (⊤ again).

use super::*;

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!s.is_empty(), "no site .{key}");
    s.iter()
        .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

// Node: prints `3 1`.
const BODY: &str = r#"
class U { a = 1; }
const us = [new U()];
class Eng {
  order(moves: readonly number[]): number[] {
    const r = moves.slice();
    r[0] = 5;
    return r;
  }
}
const e = new Eng();
"#;

#[test]
fn a_call_through_an_unrun_parameter_writes_no_builtin_prototype() {
    let src = format!("{BODY}console.log(e.order([3, 1, 2]).length, us[0].a);");
    assert!(proven(&facts(&src), "a"), "whole program");
    assert!(proven(&units(&src), "a"), "per unit as whole");
}

#[test]
fn a_call_through_a_parameter_given_an_unknown_value_stays_unknown() {
    // `process.argv` is the one array of strings the runtime makes, a known
    // value (its slice writes no builtin prototype); a value read from the
    // global object under a computed key is unknown.
    // Node: prints `3 1` (the key names no global: `undefined` is not run).
    let src = format!(
        "{BODY}const m: any = (globalThis as any)[\"hook\" + process.argv.length] ?? [3, 1, 2];\nconsole.log(e.order(m).length, us[0].a);"
    );
    // DEC 250: `r[0] = 5` into an unknown `r` is an index add on a locked
    // chain at worst, refused: `us[0]` reads its own element.
    assert!(proven(&units(&src), "a"));
}
