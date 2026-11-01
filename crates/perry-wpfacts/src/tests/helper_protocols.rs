//! Runtime helpers touch their operands with the key set ECMA-262 reads,
//! not any key: `includes`/`indexOf` read the elements and compare the
//! search value, `join` ToString's the elements, `Math.max(...xs)` iterates
//! and ToNumber's, `new Error(m)` ToString's the message. An array they
//! read does not escape; what ToString runs on an element still does.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        {body}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: prints `false 3`.
#[test]
fn includes_reads_elements_only() {
    let v = verdict("const xs = [new Svc()]; console.log(xs.includes(null as any), g(xs[0]));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `-1 3`.
#[test]
fn index_of_reads_elements_only() {
    let v = verdict("const xs = [new Svc()]; console.log(xs.indexOf(null as any), g(xs[0]));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `[object Object] 3`.
#[test]
fn join_to_strings_elements() {
    let v = verdict("const xs = [new Svc()]; console.log(xs.join(','), g(xs[0]));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `NaN 3`.
#[test]
fn math_spread_iterates_and_to_numbers() {
    let v = verdict("const xs: any[] = [new Svc()]; console.log(Math.max(...xs), g(xs[0]));");
    assert!(proven(&v), "{v:?}");
}

/// node: prints `[object Object] 3`.
#[test]
fn error_message_is_to_stringed() {
    let v = verdict("const s = [new Svc()]; console.log(new Error(s as any).message, g(s[0]));");
    assert!(proven(&v), "{v:?}");
}

/// node (`hostTake = (o) => { o.size = 'x'; }`): prints `x x`. The
/// element's own `toString` runs and hands itself to unknown code.
#[test]
fn join_runs_an_elements_to_string() {
    let v = verdict(
        "class Leak { size = 3; toString() { (globalThis as any).hostTake(this); return ''; } }
         const xs = [new Leak()]; xs.join(','); console.log(g(xs[0]));",
    );
    assert!(!proven(&v), "{v:?}");
}

/// node (`hostTake` as above): prints `x`. ToNumber runs `valueOf`.
#[test]
fn math_spread_runs_an_elements_value_of() {
    let v = verdict(
        "class Leak { size = 3; valueOf() { (globalThis as any).hostTake(this); return 1; } }
         const xs: any[] = [new Leak()]; Math.max(...xs); console.log(g(xs[0]));",
    );
    assert!(!proven(&v), "{v:?}");
}
