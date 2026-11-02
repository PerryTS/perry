//! `const hasOwnProperty = Object.prototype.hasOwnProperty;
//! hasOwnProperty.call(map, key)`: the value read is `Object.prototype`'s
//! own method (no builtin prototype overrides it), and `call` runs it on
//! `map`: an own-key test that converts the key and reads nothing else.
//! The map does not escape. Replaced, the method is the opaque member.

use super::*;

fn verdict(head: &str, test: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        {head}
        const map: any = {{}};
        map.s = new Svc();
        if ({test}) console.log(g(map.s));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const HOP: &str = "const hasOwnProperty = Object.prototype.hasOwnProperty;";

/// node: prints `3`.
#[test]
fn has_own_property_call_reads_only() {
    let v = verdict(HOP, r#"hasOwnProperty.call(map, "s")"#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `3`.
#[test]
fn has_own_property_apply_reads_only() {
    let v = verdict(HOP, r#"hasOwnProperty.apply(map, ["s"])"#);
    assert!(proven(&v), "{v:?}");
}

/// node: prints `3`.
#[test]
fn property_is_enumerable_call_reads_only() {
    let v = verdict(
        "const pie = Object.prototype.propertyIsEnumerable;",
        r#"pie.call(map, "s")"#,
    );
    assert!(proven(&v), "{v:?}");
}

/// node: prints `3`.
#[test]
fn is_prototype_of_call_reads_only() {
    let v = verdict(
        "const ipo = Object.prototype.isPrototypeOf;",
        "!ipo.call(map, map.s)",
    );
    assert!(proven(&v), "{v:?}");
}


/// node (the replacement keeps `map`): prints `3`. Replaced, the method is
/// whatever was stored: `map` reaches it.
#[test]
fn a_replaced_has_own_property_is_not_the_builtin() {
    let v = verdict(
        "(Object.prototype as any).hasOwnProperty = function (this: any, k: any) { (globalThis as any).kept = this; return true; };
         const hasOwnProperty = Object.prototype.hasOwnProperty;",
        r#"hasOwnProperty.call(map, "s")"#,
    );
    assert!(!proven(&v), "{v:?}");
}

/// node: prints `3`. A key with its own `toString` is converted (it runs).
#[test]
fn the_key_is_converted() {
    let v = verdict(
        HOP,
        r#"hasOwnProperty.call(map, { toString() { (globalThis as any).kept = map.s; return "s"; } })"#,
    );
    assert!(!proven(&v), "{v:?}");
}
