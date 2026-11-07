//! A function value of a computed-key object literal (a dispatch table,
//! `table[node.kind](node, cbNode)`) is summarized like any function: its
//! parameters are its own bindings (the literal's wrapper does not capture
//! them), so each call gets the effect over its own arguments.

use super::*;

const CLASSES: &str = r#"
    class A { x = 1; a = 1; }
    class B { y = 2; b = 2; }
"#;

fn receivers(pf: &ProgramFacts, key: &str) -> Vec<(Vec<String>, bool)> {
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| (s.receivers.clone(), s.top))
        .collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

/// Every read of `key` sees exactly `class` and no ⊤.
fn only(pf: &ProgramFacts, key: &str, class: &str) {
    for (r, top) in receivers(pf, key) {
        assert!(
            !top && r.len() == 1 && r[0].ends_with(&format!(":{class}")),
            "{key}: {r:?} top={top}\n{}",
            pf.dump_text()
        );
    }
}

#[test]
fn table_function_callers_keep_their_own_arguments() {
    let pf = facts(&format!(
        "{CLASSES}
        const table: any = {{
            [1]: function pick(node: any) {{ const p = node; return p; }},
        }};
        const a = table[1](new A());
        const b = table[1](new B());
        console.log(a.x, b.y);"
    ));
    only(&pf, "x", "A");
    only(&pf, "y", "B");
}

/// Breaking control: `map` with two callbacks, one returning `new A()` and
/// one returning `new B()`, called through a binding of the table entry
/// (plain calls). The first result never holds B.
#[test]
fn table_map_results_keep_their_own_callback() {
    let pf = facts(&format!(
        "{CLASSES}
        const fns: any = {{
            [0]: function map(xs: any[], f: (x: any) => any) {{ return f(xs[0]); }},
        }};
        const map = fns[0];
        const r1 = map([1], () => new A());
        const r2 = map([2], () => new B());
        console.log(r1.a, r2.b);"
    ));
    only(&pf, "a", "A");
    only(&pf, "b", "B");
}
