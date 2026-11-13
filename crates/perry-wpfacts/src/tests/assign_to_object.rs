//! `Object.assign(target, ...)` returns ToObject(target) (ECMA-262
//! 20.1.2.1 step 1): a primitive target is a new wrapper object, never the
//! primitive, so the result has no Number representation or interval of
//! the target's (once `rep=i32 range=[5,5]` for `Object.assign(5, ...)`).

use super::*;
use crate::FactsConfig;

fn rep_lines(src: &str) -> Vec<String> {
    let pf = facts_with(
        src,
        FactsConfig {
            diagnostics: true,
            ..Default::default()
        },
    );
    pf.rep_probe
        .iter()
        .filter(|l| l.starts_with("# rep "))
        .cloned()
        .collect()
}

fn program(init: &str) -> String {
    format!(
        r#"
const assign: any = Object.assign;
function f() {{
  const r: any = {init};
  return [typeof r, r.x, r.valueOf(), r === 5];
}}
console.log(JSON.stringify(f()));
"#
    )
}

#[test]
fn object_assign_of_a_number_is_no_number() {
    for init in ["Object.assign(5 as any, { x: 1 })", "assign(5, { x: 1 })"] {
        let ls = rep_lines(&program(init));
        assert!(!ls.iter().any(|l| l.contains("range=[5,5]")), "{init}: {ls:#?}");
    }
}

/// Control: the Number itself keeps its interval.
#[test]
fn the_number_itself_keeps_its_interval() {
    let ls = rep_lines(&program("5 as any"));
    assert!(ls.iter().any(|l| l.contains("range=[5,5]")), "{ls:#?}");
}
