//! DECISIONS 217: a prototype taken as a value is the runtime's prototype,
//! and a computed key carries its values. `__proto__` is Object.prototype's
//! accessor, found on every chain: under it (or under a computed key that
//! may be the string `"__proto__"`) a read returns the receiver's
//! prototype, as `Object.getPrototypeOf` does. A literal's prototype is
//! `Object.prototype` (its `__AnonShape` class is a layout, no object). A
//! number key names no member of an intrinsic prototype and not
//! `__proto__`, but it does name a program prototype's numeric member.

use super::*;

/// The facts see what `p.y = new E()` stored, at `q.y` of a fresh literal:
/// the read site `r.z` has `E` as a receiver (or ⊤).
fn sees_e_on_literals(get: &str) -> bool {
    let pf = facts(&format!(
        "class E {{ z = 3; }}
        const k = String(Math.random());
        const o: any = {{ a: 1 }};
        const p: any = {get};
        p.y = new E();
        const q: any = {{}};
        const r = q.y;
        if (r) console.log(r.z);"
    ));
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "z").collect();
    assert_eq!(s.len(), 1, "{}", pf.dump_text());
    s[0].top || s[0].receivers.iter().any(|r| r.ends_with(":E"))
}

#[test]
fn a_literals_prototype_as_a_value_is_object_prototype() {
    assert!(sees_e_on_literals("Object.prototype"));
    for get in [
        "o.__proto__",
        "o['__proto__']",
        "Object.getPrototypeOf(o)",
        "Reflect.getPrototypeOf(o)",
        "Reflect.get(o, '__proto__')",
        // A key that may be any string may be "__proto__".
        "o[k]",
        "Reflect.get(o, k)",
        // An array's prototype's prototype.
        "Object.getPrototypeOf(Object.getPrototypeOf([1]))",
    ] {
        assert!(sees_e_on_literals(get), "{get}");
    }
}

/// What `C`'s instances read under `y` after a store through the prototype
/// value `get` (of `c`, an instance of `C`).
fn sees_e_on_instances(get: &str) -> bool {
    let pf = facts(&format!(
        "class E {{ z = 3; }}
        class C {{ a = 1; }}
        const k = String(Math.random());
        const c: any = new C();
        const p: any = {get};
        p.y = new E();
        const q: any = new C();
        const r = q.y;
        if (r) console.log(r.z);"
    ));
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "z").collect();
    assert_eq!(s.len(), 1, "{}", pf.dump_text());
    s[0].top || s[0].receivers.iter().any(|r| r.ends_with(":E"))
}

#[test]
fn a_class_instances_prototype_as_a_value_is_its_class_prototype() {
    for get in [
        "c.__proto__",
        "Object.getPrototypeOf(c)",
        "c[k]",
        "C.prototype",
    ] {
        assert!(sees_e_on_instances(get), "{get}");
    }
}

#[test]
fn a_functions_prototype_as_a_value_is_seen_by_every_function() {
    let pf = facts(
        "class E { z = 3; }
        const f: any = function () {};
        f.__proto__.y = new E();
        const g: any = () => 1;
        const r = g.y;
        if (r) console.log(r.z);",
    );
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "z").collect();
    assert_eq!(s.len(), 1, "{}", pf.dump_text());
    assert!(
        s[0].top || s[0].receivers.iter().any(|r| r.ends_with(":E")),
        "{}",
        pf.dump_text()
    );
}

/// The receivers of the read `r.z` after `const g = <read>; const r = g();`.
fn called_receivers(read: &str) -> Vec<String> {
    let pf = facts(&format!(
        "class E {{ z = 3; }}
        class C {{ 0() {{ return new E(); }} }}
        class D extends C {{}}
        const n = Math.random() > 0.5 ? 0 : 1;
        const o: any = {{ 0() {{ return new E(); }} }};
        const d: any = new D();
        const g: any = {read};
        const r = g();
        console.log(r.z);"
    ));
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "z").collect();
    assert_eq!(s.len(), 1, "{}", pf.dump_text());
    s[0].receivers.clone()
}

#[test]
fn a_number_key_reads_the_program_prototype_chain() {
    // `D.prototype` inherits `0` from `C.prototype`: main read own fields
    // only, so `d[n]` was nothing.
    for read in ["d[n]", "o[n]", "Reflect.get(d, n)"] {
        let r = called_receivers(read);
        assert!(r.iter().any(|r| r.ends_with(":E")), "{read}: {r:?}");
    }
}

#[test]
fn a_number_key_names_no_class_member_by_name() {
    // `constructor` is no number: `d[n]` never reads a class object.
    let pf = facts(
        "class C { 0() { return 1; } }
        const n = Math.random() > 0.5 ? 0 : 1;
        const d: any = new C();
        const g: any = d[n];
        console.log(g.zz);",
    );
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "zz").collect();
    assert_eq!(s.len(), 1, "{}", pf.dump_text());
    assert!(
        !s[0].receivers.iter().any(|r| r.starts_with("class:")),
        "{:?}",
        s[0].receivers
    );
}

/// `d` escapes from `table[n](...)`'s arguments.
fn call_escapes(setup: &str, call: &str) -> bool {
    let pf = facts(&format!(
        "class D {{ y = 2; }}
        const d = new D();
        const n = Math.random() > 0.5 ? 0 : 1;
        const k = String(Math.random());
        {setup}
        {call};
        console.log(d.y);"
    ));
    tmpl(&pf, "D").escape.is_some()
}

#[test]
fn a_number_key_calls_no_object_prototype_method() {
    let table =
        "const table: any = { 0(a: any, b: any) { return 1; }, 1(a: any, b: any) { return 2; } };";
    // `__defineGetter__('y', () => d)` would install an accessor of `d`.
    assert!(!call_escapes(table, "table[n]('y', () => d)"));
    let arr = "const table: any = [(a: any, b: any) => 1];";
    assert!(!call_escapes(arr, "table[n]('y', () => d)"));
    let inst = "class T { 0(a: any, b: any) { return 1; } } const table: any = new T();";
    assert!(!call_escapes(inst, "table[n]('y', () => d)"));
    // A key that may be any string may name `__defineGetter__`.
    assert!(call_escapes(table, "table[k]('y', () => d)"));
}

#[test]
fn a_number_key_calls_what_a_store_into_an_intrinsic_wrote() {
    // A store under a computed key onto Object.prototype may write "0".
    let table = "const table: any = { a: 1 };
        (Object.prototype as any)[k] = (x: any) => (globalThis as any).sink = x;";
    assert!(call_escapes(table, "table[n](d)"));
    let s = "const s: any = 'ab';
        (String.prototype as any)[k] = (x: any) => (globalThis as any).sink = x;";
    assert!(call_escapes(s, "s[n](d)"));
}

const PROTO_OF_PARAM: &str = r#"
export function protoOf(o: any) { return o.__proto__; }
export function at(o: any, k: any) { return o[k]; }
export function getProto(o: any) { return Object.getPrototypeOf(o); }
"#;

#[test]
fn a_parameters_prototype_is_its_arguments_in_every_unit() {
    for get in ["protoOf(o)", "at(o, String(Math.random()))", "getProto(o)"] {
        let m = format!(
            "import {{ protoOf, at, getProto }} from './a';
            class E {{ z = 3; }}
            const o: any = {{ a: 1 }};
            const p: any = {get};
            p.y = new E();
            const q: any = {{}};
            const r = q.y;
            if (r) console.log(r.z);"
        );
        let mods = [("a", PROTO_OF_PARAM), ("m", m.as_str())];
        let (u, w) = super::unit_alias::assert_not_stronger(&mods);
        for (mode, pf) in [("per unit", &u), ("whole", &w)] {
            let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "z").collect();
            assert_eq!(s.len(), 1, "{get} {mode}\n{}", pf.dump_text());
            assert!(
                s[0].top || s[0].receivers.iter().any(|r| r.ends_with(":E")),
                "{get} {mode}: {:?}\n{}",
                s[0].receivers,
                pf.dump_text()
            );
        }
    }
}
