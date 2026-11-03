//! A protocol touch reads only its protocol's keys (`model::Protocol`): the
//! methods it finds there run with the touched value as `this`; any other
//! method of the value is not called and the value does not escape.

use super::*;

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::SiteFacts {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert_eq!(v.len(), 1, "site {key} in\n{}", pf.dump_text());
    v[0]
}

/// The classes a read sees as receivers (a run method's store reaches them).
fn classes(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let s = site(pf, key);
    assert!(!s.top, "{key} receiver carries ⊤\n{}", pf.dump_text());
    let mut v: Vec<String> = s
        .receivers
        .iter()
        .filter(|r| r.contains(':') && !r.starts_with("fn:"))
        .map(|r| r.rsplit(':').next().unwrap().to_string())
        .collect();
    v.sort();
    v
}

fn not_escaped(pf: &ProgramFacts, name: &str) {
    let t = tmpl(pf, name);
    assert!(
        t.escape.is_none(),
        "{name} escapes: {:?}\n{}",
        t.escape,
        pf.dump_text()
    );
}

#[test]
fn to_primitive_runs_a_user_value_of() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class V {
            s: any;
            constructor() { this.s = undefined; }
            valueOf() { this.s = new A(); return 1; }
        }
        const x = new V();
        console.log(x + "");
        console.log(x.s.a);
        "#,
    );
    assert_eq!(classes(&pf, "a"), ["A"], "{}", pf.dump_text());
}

#[test]
fn to_primitive_runs_a_user_symbol_to_primitive() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class V {
            s: any;
            constructor() { this.s = undefined; }
            [Symbol.toPrimitive](hint: string) { this.s = new A(); return 1; }
        }
        const x = new V();
        console.log(`${x}`);
        console.log(x.s.a);
        "#,
    );
    assert_eq!(classes(&pf, "a"), ["A"], "{}", pf.dump_text());
}

#[test]
fn to_primitive_calls_no_other_method() {
    // `foo` is no ToPrimitive key: a touch under any key runs it (its store
    // reaches `f.s`) and calls every field value with `this` = f.
    let pf = facts(
        r#"
        class B { b = 2; }
        class F {
            s: any;
            constructor() { this.s = undefined; }
            foo(p: any) { this.s = new B(); return p; }
        }
        const f = new F();
        console.log(f + "", `${f}`, f < 1);
        console.log(f.s.b);
        "#,
    );
    assert_eq!(
        classes(&pf, "b"),
        Vec::<String>::new(),
        "{}",
        pf.dump_text()
    );
    not_escaped(&pf, "F");
}

#[test]
fn spread_iteration_runs_the_iterator_only() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        class It {
            s: any; t: any;
            constructor() { this.s = undefined; this.t = undefined; }
            // Before the computed member: a member after one is computed
            // too (source order), stored under any key.
            foo() { this.t = new B(); }
            [Symbol.iterator]() {
                this.s = new A();
                let i = 0;
                return { next: () => i++ < 1 ? { value: 1, done: false } : { value: undefined, done: true } };
            }
        }
        function g(...xs: any[]) { return xs.length; }
        const it = new It();
        console.log(g(...it));
        console.log(it.s.a, it.t.b);
        "#,
    );
    assert_eq!(classes(&pf, "a"), ["A"], "{}", pf.dump_text());
    assert_eq!(
        classes(&pf, "b"),
        Vec::<String>::new(),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn thenable_adoption_calls_then_only() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        class T {
            s: any; t: any;
            constructor() { this.s = undefined; this.t = undefined; }
            then(res: any) { this.s = new A(); res(1); }
            foo() { this.t = new B(); }
        }
        const th = new T();
        async function main() { await th; console.log(th.s.a, th.t.b); }
        main();
        "#,
    );
    assert_eq!(classes(&pf, "a"), ["A"], "{}", pf.dump_text());
    assert_eq!(
        classes(&pf, "b"),
        Vec::<String>::new(),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn json_runs_nested_to_json_only() {
    // SerializeJSONProperty runs `toJSON` on every serialized value, the
    // nested one included; `foo` is not a JSON key.
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        class In {
            s: any; t: any;
            constructor() { this.s = undefined; this.t = undefined; }
            toJSON() { this.s = new A(); return 1; }
            foo() { this.t = new B(); }
        }
        const inner = new In();
        const o = { inner };
        console.log(JSON.stringify(o));
        console.log(inner.s.a, inner.t.b);
        "#,
    );
    assert_eq!(classes(&pf, "a"), ["A"], "{}", pf.dump_text());
    assert_eq!(
        classes(&pf, "b"),
        Vec::<String>::new(),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn own_keys_call_nothing() {
    // `Object.keys`, for-in and `Object.getOwnPropertyNames` run no lookup
    // on an ordinary object: not even the ToPrimitive keys.
    let pf = facts(
        r#"
        class B { b = 2; }
        class K {
            s: any;
            constructor() { this.s = undefined; }
            valueOf() { this.s = new B(); return 1; }
            toString() { this.s = new B(); return "k"; }
        }
        const k = new K();
        console.log(Object.keys(k).length, Object.getOwnPropertyNames(k).length);
        for (const x in k) console.log(x);
        console.log(k.s.b);
        "#,
    );
    assert_eq!(
        classes(&pf, "b"),
        Vec::<String>::new(),
        "{}",
        pf.dump_text()
    );
    not_escaped(&pf, "K");
}

#[test]
fn instanceof_runs_has_instance_only() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = 2; }
        const h = {
            s: undefined as any, t: undefined as any,
            [Symbol.hasInstance](v: any) { this.s = new A(); return true; },
            foo() { this.t = new B(); },
        };
        console.log(({}) instanceof (h as any));
        console.log(h.s.a, h.t.b);
        "#,
    );
    assert_eq!(classes(&pf, "a"), ["A"], "{}", pf.dump_text());
    assert_eq!(
        classes(&pf, "b"),
        Vec::<String>::new(),
        "{}",
        pf.dump_text()
    );
}
