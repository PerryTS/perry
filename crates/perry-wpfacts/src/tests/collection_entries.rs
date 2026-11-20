//! A Map or Set holds its entries in internal slots, not in properties:
//! `map.get` reads `Map.prototype.get`, never a value the map holds. A
//! property stored on the map under a computed key is a property, and a
//! named read may see it.

use super::*;

fn verdict(tail: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        function use(mm: any, x: any) {{ return mm.get(x); }}
        const s = new Svc();
        const m = new Map<string, any>();
        m.set("h", (x: any) => {{ (globalThis as any).hostTake(x); }});
        {tail}
        console.log(g(s));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: `hostTake` is never called (`get` returns undefined; the held function never runs).
#[test]
fn a_held_value_is_no_method_of_the_map() {
    let v = verdict("use(m, s);");
    assert!(proven(&v), "{v:?}");
}

/// node: `hostTake` gets `s` (the held function runs on it).
#[test]
fn a_held_value_read_out_still_runs() {
    let v = verdict("const f = use(m, \"h\"); f(s);");
    assert!(!proven(&v), "{v:?}");
}

/// node: `hostTake` gets `s` (an own `get` property shadows `Map.prototype.get`).
#[test]
fn a_computed_property_store_still_shadows() {
    let v = verdict("const k = [\"get\"][0]; (m as any)[k] = (x: any) => { (globalThis as any).hostTake(x); }; use(m, s);");
    assert!(!proven(&v), "{v:?}");
}
