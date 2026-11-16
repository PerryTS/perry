//! A composed call binds its spread entry at the elements only: the
//! spread list of `inner(...q)` is CreateListFromIterable (13.3.8.1), a
//! List of the iterated values with no prototype, so the callee rest
//! array never holds a member of the Array prototype chain
//! (`Array.prototype.slice`, `Object.prototype.toString`). A call of
//! `p[0]` is then no call of an opaque built-in function, which would hand
//! `b` to unseen code (three.js warnOnce `warn(...params)`).

use super::*;

const SRC: &str = r#"
class Box { v = 1; w = 2; }
const b = new Box();
const ys = [1, 2].slice(0);
function inner(...p: any[]) {
    const m = p[0];
    return typeof m === "function" ? m(b) : 0;
}
function outer(...q: any[]) {
    return inner(...q);
}
function run() { return outer(ARGS); }
console.log(run(), b.w);
"#;

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

#[test]
fn a_composed_spread_entry_supplies_its_elements_only() {
    let pf = facts(&SRC.replace("ARGS", "\"a\", 1"));
    assert_eq!(verdicts(&pf, "w"), ["T0Mono"], "\n{}", pf.dump_text());
}

/// Control: a spread of a value of unknown origin still supplies unknown
/// functions to the rest parameter, so `b` reaches unseen code.
#[test]
fn a_composed_spread_of_an_unknown_value_still_supplies_unknown_functions() {
    let src = SRC
        .replace("return inner(...q);", "return inner(...(globalThis as any)[String(q.length)]);")
        .replace("ARGS", "\"a\", 1");
    let pf = facts(&src);
    assert!(
        verdicts(&pf, "w").iter().all(|v| !v.starts_with("T0")),
        "\n{}",
        pf.dump_text()
    );
}
