//! A Map iterator yields CreateArrayFromList(« key, value ») (ECMA-262
//! 24.1.5.1): a fresh Array whose own index 0 is the key and index 1 the
//! value. Its pair is index-exact (`Model::index_slot`): `pair[1]` under the
//! constant key 1 reads the value only, `pair[0]` the key only, while
//! nothing stored into the pair, moved its elements or handed it to unknown
//! code. The map is iterated as an untyped value, so the loop calls its
//! `@@iterator` (a typed Map loop lowers to the HIR entry views).

use super::*;

/// The sink takes `s` only through a wrong pair read (`map_values` shape).
fn verdict(tail: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const s = new Svc();
        const unknown: any = (globalThis as any)[process.argv[2]];
        const take = (x: any) => {{ unknown.take(x); }};
        (function () {{ {tail} }})();
        console.log(g(s));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: `v` is the value `{ b: 2 }`, never the key `s`.
#[test]
fn destructured_value_is_the_value_only() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const [k, v] of x) take(v); }; walk(m);",
    );
    assert!(proven(&v), "{v:?}");
}

/// node: `k` is the key `{ b: 2 }`, never the value `s`.
#[test]
fn destructured_key_is_the_key_only() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set({ b: 2 }, s); \
         const walk = (x: any) => { for (const [k, v] of x) take(k); }; walk(m);",
    );
    assert!(proven(&v), "{v:?}");
}

/// node: `e[1]` of the entry is the value.
#[test]
fn indexed_pair_reads_its_index() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const e of x) { take(e[1]); } }; walk(m);",
    );
    assert!(proven(&v), "{v:?}");
}

/// node: `entries()` yields the same pairs (24.1.3.4).
#[test]
fn entries_pairs_are_exact() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const [k, v] of x.entries()) take(v); }; walk(m);",
    );
    assert!(proven(&v), "{v:?}");
}

/// Control (already exact): forEach passes the value first (24.1.3.5).
#[test]
fn foreach_value_control() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { x.forEach((v: any, k: any) => take(v)); }; walk(m);",
    );
    assert!(proven(&v), "{v:?}");
}

/// Control: a Map handed to unknown code may hold any entry.
#[test]
fn escaped_map_stays_open() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); new Function(process.argv[2])(m); \
         const walk = (x: any) => { for (const [k, v] of x) take(v); }; walk(m);",
    );
    assert!(!proven(&v), "{v:?}");
}

/// Control: a Map unknown code made.
#[test]
fn unknown_map_stays_open() {
    let v = verdict(
        "const m: any = new Function(process.argv[2])(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const [k, v] of x) take(v); }; walk(m);",
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: `reverse` moves the key to index 1.
#[test]
fn reversed_pair_holds_both() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const e of x) { e.reverse(); take(e[1]); } }; walk(m);",
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: a key that is not one constant may be 0.
#[test]
fn unknown_index_reads_both() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const e of x) { take(e[process.argv.length]); } }; walk(m);",
    );
    assert!(!proven(&v), "{v:?}");
}

/// The rest element is a new array iterated from the pair: merged as today.
#[test]
fn rest_pattern_stays_merged() {
    let v = verdict(
        "const m = new Map<any, any>(); m.set(s, { b: 2 }); \
         const walk = (x: any) => { for (const [...r] of x) take(r[1]); }; walk(m);",
    );
    assert!(!proven(&v), "{v:?}");
}
