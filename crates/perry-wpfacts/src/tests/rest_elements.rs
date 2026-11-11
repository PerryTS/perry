//! A rest parameter is the array the call makes from its arguments at
//! indices 0..n: the binding is an element store. An argument is never a
//! named key of the rest array (its `@@iterator`, its `indexOf`), so
//! iterating the rest array or calling a builtin method on it calls no
//! argument with the array as `this` (EUC2a: tssrc path.ts / program.ts).

use super::*;

const REST: &str = r#"
class Box { v = 1; w = 2; }
function combine(base: string, ...parts: any[]): number {
  let n = base.length;
  for (const p of parts) n += typeof p === "function" ? 1 : (p as Box).v;
  return n + parts.indexOf(base);
}
const b = new Box();
const d = new Date();
console.log(combine("a", b, d.getTime), b.w);
"#;

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

#[test]
fn a_builtin_function_argument_is_no_method_of_the_rest_array() {
    let pf = facts(REST);
    assert_eq!(verdicts(&pf, "w"), ["T0Mono"], "\n{}", pf.dump_text());
}

/// The same through a spread forward (`resolve(...) -> combine(base, ...parts)`).
#[test]
fn a_spread_forward_into_a_rest_parameter_stores_elements_only() {
    let src = REST.replace(
        "console.log(combine(\"a\", b, d.getTime), b.w);",
        "function resolve(base: string, ...parts: any[]): number { return parts.length ? combine(base, ...parts) : 0; }\nconsole.log(resolve(\"a\", b, d.getTime), b.w);",
    );
    let pf = facts(&src);
    assert_eq!(verdicts(&pf, "w"), ["T0Mono"], "\n{}", pf.dump_text());
}

/// The rest array handed to unknown code still escapes with its elements.
#[test]
fn a_rest_array_given_to_unknown_code_escapes_its_elements() {
    let src = REST.replace(
        "let n = base.length;",
        "let n = base.length; (globalThis as any)[String(base.length)](parts);",
    );
    let pf = facts(&src);
    assert!(
        verdicts(&pf, "w").iter().all(|v| !v.starts_with("T0")),
        "\n{}",
        pf.dump_text()
    );
}
