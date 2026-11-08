//! `Object.values(o)` / `Object.entries(o)` read the values of `o`'s own
//! enumerable properties (EnumerableOwnProperties: the keys of
//! [[OwnPropertyKeys]], then a [[Get]] of each), never an inherited member:
//! Object.prototype's `constructor` (`Object`) and methods are not among
//! them. tsc's checker.ts: `new Map(Object.entries({ ... }))` must not
//! make `Object` a possible value of the map, or a computed store into one
//! of its values (`setCachedIterationTypes`, `type[cacheKey] = v`) reads as
//! `Object.__proto__ = v` and sinks `v`'s members (E6).

use super::*;

/// `got` is one value `{read}` takes; a computed store of `holder` into it
/// may be a `__proto__` store when `got` may be a builtin object.
fn inner_escape(read: &str) -> Option<String> {
    let pf = facts(&format!(
        r#"
        class Inner {{ n = 1; }}
        class Holder {{ inner = new Inner(); }}
        const got: any = {read};
        function setCached(type: any, key: string, v: any) {{ type[key] = v; }}
        const holder = new Holder();
        setCached(got, String(Math.random()), holder);
        console.log(holder.inner.n);
        "#
    ));
    tmpl(&pf, "Inner").escape.clone()
}

#[test]
fn object_values_holds_no_inherited_member() {
    assert_eq!(inner_escape("Object.values({ a: { k: 1 } })[0]"), None);
}

#[test]
fn object_entries_holds_no_inherited_member() {
    assert_eq!(inner_escape("Object.entries({ a: { k: 1 } })[0][1]"), None);
}

#[test]
fn object_values_as_a_value_call_holds_no_inherited_member() {
    assert_eq!(
        inner_escape("(0, Object.values)({ a: { k: 1 } })[0]"),
        None
    );
}

/// Control: an own value that is `Object` itself is still a value, and the
/// store into it may set `Object.__proto__`.
#[test]
fn an_own_value_that_is_a_builtin_still_gives_a_prototype() {
    let e = inner_escape("Object.values({ a: Object })[0]");
    assert!(e.as_deref().is_some_and(|e| e.contains("E6")), "{e:?}");
}

/// Control: a computed read is a [[Get]] that walks the chain; it may find
/// Object.prototype's members, and the store into one gives it a prototype.
#[test]
fn a_computed_read_still_finds_inherited_members() {
    let e = inner_escape("({ a: { k: 1 } } as any)[String(Math.random())]");
    assert!(e.as_deref().is_some_and(|e| e.contains("E6")), "{e:?}");
}
