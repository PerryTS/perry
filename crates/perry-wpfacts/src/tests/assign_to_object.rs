//! `Object.assign(target, ...)` returns ToObject(target) (ECMA-262
//! 20.1.2.1 step 1): a primitive target is a new wrapper object, never the
//! primitive, so the result has no Number representation or interval of
//! the target's (once `rep=i32 range=[5,5]` for `Object.assign(5, ...)`),
//! and it holds the wrapper. An object target is the result as it is.

use super::*;
use crate::FactsConfig;

fn facts_diag(src: &str) -> ProgramFacts {
    facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    )
}

fn rep_lines(pf: &ProgramFacts) -> Vec<String> {
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rep "))
        .cloned()
        .collect()
}

/// The receivers of the read sites of `key`, and whether one holds ⊤.
fn receivers(pf: &ProgramFacts, key: &str) -> (Vec<String>, bool) {
    let ss: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!ss.is_empty(), "site {key} in\n{}", pf.dump_text());
    let mut got: Vec<String> = ss.iter().flat_map(|s| s.receivers.clone()).collect();
    got.sort();
    got.dedup();
    (got, ss.iter().any(|s| s.top))
}

fn program(init: &str, key: &str) -> String {
    format!(
        r#"
const assign: any = Object.assign;
function f() {{
  const r: any = {init};
  return [typeof r, r.{key}, r.valueOf(), r === 5];
}}
console.log(JSON.stringify(f()));
"#
    )
}

#[test]
fn object_assign_of_a_number_is_its_wrapper() {
    for (init, key) in [
        ("Object.assign(5 as any, { px: 1 })", "px"),
        ("assign(5, { py: 1 })", "py"),
    ] {
        let pf = facts_diag(&program(init, key));
        let ls = rep_lines(&pf);
        assert!(
            !ls.iter().any(|l| l.contains("range=[5,5]")),
            "{init}: {ls:#?}"
        );
        // The read of the copied key is on the wrapper, and only on it.
        let (got, top) = receivers(&pf, key);
        assert!(
            !top && got.len() == 1 && got[0].contains("Object("),
            "{init}: {got:?} top={top}\n{}",
            pf.dump_text()
        );
    }
}

/// Control: the Number itself keeps its interval.
#[test]
fn the_number_itself_keeps_its_interval() {
    let ls = rep_lines(&facts_diag(&program("5 as any", "px")));
    assert!(ls.iter().any(|l| l.contains("range=[5,5]")), "{ls:#?}");
}

/// An object target is the result itself: its token alone, no wrapper.
#[test]
fn object_assign_of_an_object_is_the_object() {
    for init in ["Object.assign(o, { x: 1 })", "assign(o, { x: 1 })"] {
        let pf = facts_diag(&format!(
            r#"
const assign: any = Object.assign;
class K {{ k = 1; }}
const o = new K();
const r: any = {init};
console.log(r.k, r === o);
"#
        ));
        let (got, top) = receivers(&pf, "k");
        assert!(
            !top && got == vec!["t.ts:K".to_string()],
            "{init}: {got:?} top={top}\n{}",
            pf.dump_text()
        );
    }
}
