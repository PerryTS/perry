//! A computed key that is a call of a module function is a String its body
//! returns (`KeyShape::of_returns`). tsc's checker.ts reads
//! `iterationTypes[getIterationTypesKeyFromIterationTypeKind(kind)]`, whose
//! function returns only `"yieldType"`, `"returnType"`, `"nextType"` (or
//! `undefined` past its switch): the read names no member of
//! Object.prototype. As an any-key read it found them, a computed store
//! into the value read (`setCachedIterationTypes`, `type[cacheKey] = v`)
//! became a possible `__proto__` store (E6), and a possible store of `call`
//! on `Object.prototype.hasOwnProperty`, which opened the own-property
//! guard of `extend()` (`hasOwnProperty.call(second, id)`).

use super::*;

/// The escape of `Inner`, reachable from a value stored by a computed key
/// into `got`.
fn esc(prelude: &str, read: &str) -> Option<String> {
    let pf = facts(&format!(
        r#"
        class Inner {{ n = 1; }}
        class Holder {{ inner = new Inner(); }}
        const hasOwnProperty = Object.prototype.hasOwnProperty;
        const it: any = {{ yieldType: {{ k: 1 }}, returnType: {{ k: 2 }}, nextType: {{ k: 3 }} }};
        {prelude}
        const got: any = {read};
        function setCached(type: any, key: string, v: any) {{ type[key] = v; }}
        const holder = new Holder();
        setCached(got, String(Math.random()), holder);
        console.log(holder.inner.n);
        "#
    ));
    tmpl(&pf, "Inner").escape.clone()
}

/// A key function in the shape of tsc's: a switch of string returns.
fn key_fn(returns: &[&str]) -> String {
    let cases: String = returns
        .iter()
        .enumerate()
        .map(|(i, r)| format!("case {i}: return {r}; "))
        .collect();
    format!("function keyOf(k: number) {{ switch (k) {{ {cases} }} }}")
}

const READ: &str = "it[keyOf(Math.random() > 0.5 ? 1 : 0)]";

#[test]
fn a_key_function_of_string_returns_names_no_inherited_member() {
    let f = key_fn(&["\"yieldType\"", "\"returnType\"", "\"nextType\""]);
    assert_eq!(esc(&f, READ), None);
}

/// A returned template or concatenation is its glob.
#[test]
fn a_key_function_of_shaped_returns_names_no_inherited_member() {
    let f = key_fn(&["\"yield\" + \"Type\"", "`${k}Type`"]);
    assert_eq!(esc(&f, READ), None);
}

/// A nested function returns to its own caller: its returns are not the
/// key function's.
#[test]
fn a_nested_return_is_not_the_key() {
    let f = "function keyOf(k: number) { const g = () => String(k); g(); \
             if (k) return \"yieldType\"; return \"nextType\"; }";
    assert_eq!(esc(f, READ), None);
}

/// tsc's cycle: the guarded copy in `extend` stays an own read when no
/// computed store may write `hasOwnProperty.call`.
#[test]
fn a_shaped_key_keeps_the_own_guard_closed() {
    let pre = format!(
        "{} function extend(first: any, second: any) {{ const result: any = {{}}; \
         for (const id in second) {{ if (hasOwnProperty.call(second, id)) {{ result[id] = second[id]; }} }} \
         for (const id in first) {{ if (hasOwnProperty.call(first, id)) {{ result[id] = first[id]; }} }} \
         return result; }} \
         const src: any = {{ b: {{ k: 2 }} }}; src[String(Math.random())] = {{ k: 3 }}; \
         const v0: any = {READ}; v0[String(Math.random())] = 7;",
        key_fn(&["\"yieldType\"", "\"returnType\"", "\"nextType\""])
    );
    assert_eq!(esc(&pre, "extend({ a: { k: 1 } }, src).b"), None);
}

/// Control: a return of an unknown string keeps the any-key read.
#[test]
fn a_key_function_with_an_unknown_return_still_finds_inherited_members() {
    let f = key_fn(&["\"yieldType\"", "String(Math.random())"]);
    let e = esc(&f, READ);
    assert!(e.as_deref().is_some_and(|e| e.contains("E6")), "{e:?}");
}

/// Control: a returned name of an Object.prototype member is read there.
#[test]
fn a_key_function_returning_constructor_finds_object() {
    let f = key_fn(&["\"yieldType\"", "\"constructor\""]);
    let e = esc(&f, READ);
    assert!(e.as_deref().is_some_and(|e| e.contains("E6")), "{e:?}");
}

/// Control: an async function's call is a promise, no String.
#[test]
fn an_async_key_function_has_no_shape() {
    let f = "async function keyOf(k: number) { return \"yieldType\"; }";
    let e = esc(f, "it[keyOf(1) as any]");
    assert!(e.as_deref().is_some_and(|e| e.contains("E6")), "{e:?}");
}
