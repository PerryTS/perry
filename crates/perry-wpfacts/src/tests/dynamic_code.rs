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
