//! DECISIONS 213: dynamic code is a call whose callee may be the intrinsic
//! Function constructor (or `eval`, `node:vm`) itself. A read finds it as
//! the `constructor` of `Function.prototype`, which only a function's
//! prototype chain reaches: a computed read off an object literal's chain
//! finds `Object` and `Object.prototype`'s methods, no dynamic code.

use super::*;

const PRELUDE: &str = r#"
    class D { y = 2; }
    const d = new D();
    const k = String(Math.random());
"#;

fn d_escape(body: &str) -> Option<String> {
    let pf = facts(&format!("{PRELUDE}{body}\nconsole.log(d.y);"));
    tmpl(&pf, "D").escape.clone()
}

/// The facts conclude that the Function constructor may run.
fn dynamic(body: &str) -> bool {
    let pf = facts_with(&format!("{PRELUDE}{body}\nconsole.log(d.y);"), interp_cfg());
    pf.interp_guard.as_deref().is_some_and(|g| g.contains("Function(...)"))
}

#[test]
fn dispatch_table_read_is_no_dynamic_code() {
    // `forEachChildTable[node.kind](...)`: the table's functions, or what
    // `Object.prototype` holds under the key.
    let table = "const table: any = { a(o: any) { return o.y; }, b(o: any) { return o.y; } };";
    assert_eq!(
        d_escape(&format!("{table} const fn = table[k]; if (fn !== undefined) fn(d);")),
        None
    );
    assert!(!dynamic(&format!("{table} const fn = table[k]; if (fn !== undefined) fn(d);")));
    // As a method: `Object.prototype`'s methods run on the table.
    assert_eq!(d_escape(&format!("{table} table[k](d);")), None);
    assert!(!dynamic(&format!("{table} table[k](d);")));
    // An array's `constructor` is `Array`.
    assert!(!dynamic("const a: any = [1]; const C = a.constructor; C(d);"));
    assert!(!dynamic("const a: any = [1]; a.constructor(d);"));
    // Under a computed key: `Array.prototype`'s members, no Function.
    assert!(!dynamic("const a: any = [1]; const g = a[k]; g(d);"));
    assert!(!dynamic("const a: any = [1]; a[k](d);"));
}

#[test]
fn function_constructor_is_dynamic_code() {
    assert!(dynamic("new Function('o', 'o.x = 1')(d); const F: any = Function; F(d);"));
    assert!(dynamic("const F: any = Function; F(d);"));
    // A function's `constructor`, also under a computed key.
    assert!(dynamic("const g: any = function () {}; g.constructor(d);"));
    assert!(dynamic("const g: any = function () {}; const h = g[k]; h(d);"));
    assert!(d_escape("const g: any = function () {}; g[k](d);").is_some());
    assert!(dynamic("class A {} (A as any)[k](d);"));
    // A builtin constructor is a function: its `constructor` is Function.
    assert!(dynamic("const O: any = Object; O.constructor(d);"));
    assert!(dynamic("const O: any = Object; const c = O.constructor; c(d);"));
    assert!(dynamic("const O: any = Object; O[k](d);"));
    assert!(dynamic("const t: any = { a: 1 }; const c = t[k]; const f = c[k]; f(d);"));
}

#[test]
fn prototype_given_as_a_value_may_be_function_prototype() {
    assert!(dynamic("const p: any = { __proto__: Function.prototype }; p.constructor(d);"));
    assert!(dynamic(
        "const q: any = {}; Object.setPrototypeOf(q, Function.prototype); q.constructor(d);"
    ));
    assert!(dynamic("const c: any = Object.create(Function.prototype); c.constructor(d);"));
    assert!(dynamic(
        "function F(this: any) {} F.prototype = Function.prototype; const f: any = new (F as any)(); f.constructor(d);"
    ));
}

#[test]
fn object_prototype_method_value_runs_on_its_receiver() {
    // The value may be `__defineGetter__`: called on `h` it installs an
    // accessor there.
    let pf = facts(
        r#"
        class H { z = 1; }
        const k = String(Math.random());
        const h = new H();
        const m: any = ({} as any)[k];
        (h as any).m = m;
        (h as any).m("y", () => 1);
        console.log(h.z);
        "#,
    );
    assert!(tmpl(&pf, "H").escape.is_some(), "{}", pf.dump_text());
    // Its key is converted, its accessor function escapes.
    assert_eq!(d_escape("const t: any = {}; t[k](d);"), None);
    assert!(d_escape("const t: any = {}; t[k]('y', () => d);").is_some());
}

/// The family a chain's `ObjectProto` stands for answers with that
/// family's members (`members`) and the written-key rule beside them: a
/// store into ⊤ under `constructor`, or under any key for a computed read,
/// may have replaced the member, before or after the read in the solve.
/// A literal has no own `constructor` (its shape class's prototype does not
/// declare one; was: the lookup stopped there and read nothing, a wrong
/// proof); a class instance's is its class's own, which no store into ⊤
/// replaces.
#[test]
fn a_store_into_top_reaches_the_seeded_chains() {
    let f = "function (o: any) { o.y = 'x'; }";
    let named = format!("t.constructor = {f};");
    let computed = format!("declare const kk: string; t[kk] = {f};");
    let elsewhere = format!("t.unrelated = {f};");
    let cases = |recv: &str, read: &str| {
        let r = format!("const o: any = {recv}; {read}");
        let mut v = Vec::new();
        for (w, by_name) in [(&named, true), (&computed, true), (&elsewhere, false)] {
            v.push((format!("{TOP}{w} {r}"), by_name));
            v.push((format!("{TOP}{r} {w}"), by_name));
        }
        v
    };
    for recv in ["[1]", "{ a: 1 }", "{}", "{ f() { return 1; } }"] {
        for read in ["o.constructor(d);", "const C = o.constructor; C(d);"] {
            assert_eq!(d_escape(&format!("{TOP}const o: any = {recv}; {read}")), None);
            for (body, want) in cases(recv, read) {
                assert_eq!(d_escape(&body).is_some(), want, "{body}");
            }
        }
    }
    for (body, _) in cases("new (class P {})()", "o.constructor(d);") {
        assert_eq!(d_escape(&body), None, "{body}");
    }
    for recv in ["{ a: 1 }", "new (class P {})()"] {
        assert_eq!(d_escape(&format!("{TOP}const o: any = {recv}; o[k](d);")), None);
        for (body, _) in cases(recv, "o[k](d);") {
            assert!(d_escape(&body).is_some(), "{body}");
        }
    }
}

const TOP: &str = "declare const unknown: any; const t: any = unknown.reach(0);";

/// Open (BFI census, 6,450 sites in the all-knobs-off arm): a key whose
/// values are all numbers names no `Object.prototype` method (no intrinsic
/// prototype has a numeric member), so `table[n](...)` cannot run
/// `__defineGetter__` and sink its accessor argument. The model has no key
/// values: every index that is no string literal is `Key::Any` (`key_of`).
/// Closing it needs the key's values on the load / call (ToPropertyKey of
/// its primitives), not its type.
#[test]
#[ignore = "open: computed keys carry no values (numeric key reads Object.prototype methods)"]
fn a_numeric_key_names_no_object_prototype_method() {
    let table = "const table: any = { 0(a: any, b: any) { return 1; }, 1(a: any, b: any) { return 2; } };";
    let n = "const n = Math.random() > 0.5 ? 0 : 1;";
    assert_eq!(d_escape(&format!("{table} {n} table[n]('y', () => d);")), None);
}
