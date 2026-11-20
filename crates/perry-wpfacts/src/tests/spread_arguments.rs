//! A spread argument of a built-in call supplies its elements, as a
//! program callee's parameters read them (an element read), never a
//! member a computed key could name: spreading an array into `unshift` or
//! into a declared native (`push`) adds no built-in method of the array
//! (its `indexOf`, its `@@iterator`) to the receiver's elements. A call of
//! such an element is then no call of an opaque built-in function, which
//! would hand its arguments to unseen code (tssrc checker.ts
//! `targets[i]()`). With a spread, a declared native's positional argument
//! may be any element the spread supplies (`fill(...xs)`).

use super::*;

const SRC: &str = r#"
class Box { v = 1; w = 2; }
const b = new Box();
const fs: Array<(x: Box) => number> = [(x) => x.v];
const xs: Array<(x: Box) => number> = [];
xs.METHOD(...SPREAD);
console.log(xs[0](b), b.w);
"#;

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

fn src(method: &str, spread: &str) -> String {
    SRC.replace("METHOD", method).replace("SPREAD", spread)
}

#[test]
fn an_array_spread_into_unshift_adds_its_elements_only() {
    let pf = facts(&src("unshift", "fs"));
    assert_eq!(verdicts(&pf, "w"), ["T0Mono"], "\n{}", pf.dump_text());
}

#[test]
fn an_array_spread_into_a_declared_native_adds_its_elements_only() {
    let pf = facts(&src("push", "fs"));
    assert_eq!(verdicts(&pf, "w"), ["T0Mono"], "\n{}", pf.dump_text());
}

/// Control: a spread of a value of unknown origin still supplies unknown
/// functions, so `b` reaches unseen code.
#[test]
fn a_spread_of_an_unknown_value_still_supplies_unknown_functions() {
    for method in ["unshift", "push"] {
        let pf = facts(&src(method, "(globalThis as any)[String(b.v)]"));
        assert!(
            verdicts(&pf, "w").iter().all(|v| !v.starts_with("T0")),
            "{method}\n{}",
            pf.dump_text()
        );
    }
}

/// `fill(...[g])`: the spread supplies the fill value; `xs[0]` may be `g`,
/// which then reads `z` of a `Cup`.
#[test]
fn a_spread_supplies_a_positional_argument_of_a_declared_native() {
    let src = r#"
class Cup { z = 3; v = 4; }
const g = (x: any) => x.z;
const xs: Array<(x: any) => number> = [(x) => x.v];
xs.fill(...[g]);
console.log(xs[0](new Cup()));
"#;
    let pf = facts(src);
    let z: Vec<_> = pf.sites.iter().filter(|s| s.key == "z").collect();
    assert!(
        !z.is_empty()
            && z.iter()
                .all(|s| s.receivers.iter().any(|r| r.ends_with("Cup"))),
        "\n{}",
        pf.dump_text()
    );
}
