//! A spread argument yields the iterated elements (CreateListFromIterable),
//! never a method a computed key could name: spreading an array passes its
//! elements, not an opaque builtin function. A spread of a value from
//! unmodeled code passes ⊤.

use super::*;

fn verdict(spread: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        {spread}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: prints `3`.
#[test]
fn a_spread_array_passes_its_elements() {
    let v = verdict("const xs = [new Svc()]; console.log(g(...xs));");
    assert!(proven(&v), "{v:?}");
}

/// node (`hostList = [{ size: 9 }]`): prints `9`.
#[test]
fn a_spread_top_value_passes_top() {
    let v = verdict("const xs: any = (globalThis as any).hostList; console.log(g(...xs));");
    assert!(!proven(&v), "{v:?}");
}
