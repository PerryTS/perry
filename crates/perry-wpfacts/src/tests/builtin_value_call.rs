//! VALCALL: a call of a builtin `Object` static as a VALUE (ECMA-262
//! 20.1.2.2 create, 20.1.2.3 defineProperties, 20.1.2.4 defineProperty,
//! 20.1.2.7 fromEntries, 20.1.2.9 getOwnPropertyDescriptor, 20.1.2.10
//! getOwnPropertyDescriptors, 20.1.2.13 groupBy, 20.1.2.23 setPrototypeOf)
//! runs the steps of the direct call: no unknown code runs, so the realm is
//! not held. three.js skin: `this.copySampleValue_(i1 - 1)` in
//! Interpolant.evaluate has these statics in its callee set (PropertyBinding's
//! any-key store); calling them as unknown code held the whole realm.
//! `run` gives (held, escapes): `held` = V (sound1.rs probe: a program object
//! put on a builtin through a JSON key) is not proven, which holds iff unknown
//! code holds the realm; `escapes` = `o.marker` is not proven.
use super::*;

fn run(body: &str) -> (bool, bool) {
    let src = format!(
        r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        class Svc {{ marker = 3; }}
        function g(a: any) {{ return a.marker; }}
        function pick(x: any) {{ return x; }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        const cache: any = {{}};
        const k = json.nodes[0];
        const node: any = cache[k] !== undefined ? cache[k] : {{ userData: {{}} }};
        if (node.userData.pivot !== undefined) {{ node.pivot = new V(); }}
        const o: any = new Svc();
        {body}
        console.log(g(o));
        console.log(new V().probe());
        "#
    );
    let pf = facts(&src);
    (
        matches!(site(&pf, "zz").verdict, Verdict::No(_)),
        matches!(site(&pf, "marker").verdict, Verdict::No(_)),
    )
}

fn proven(body: &str, key: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ marker = 3; }}
        function pick(x: any) {{ return x; }}
        {body}
        "#
    );
    !matches!(site(&facts(&src), key).verdict, Verdict::No(_))
}

const VALUE_CALLS: &[&str] = &[
    "const f: any = pick(Object.create); const c = f(o); console.log(c);",
    "const f: any = pick(Object.create); const c = f(null, { a: { value: o } }); console.log(c);",
    "const f: any = pick(Object.setPrototypeOf); const t: any = {}; f(t, o);",
    "const f: any = pick(Object.defineProperty); const t: any = {}; f(t, 'k', { value: o }); console.log(t);",
    "const f: any = pick(Object.defineProperty); const t: any = {}; f(t, 'k', { get() { return o; } }); console.log(t.k);",
    "const f: any = pick(Object.defineProperties); const t: any = {}; f(t, { k: { value: o } }); console.log(t);",
    "const f: any = pick(Object.getOwnPropertyDescriptor); const d = f({ k: o }, 'k'); console.log(d);",
    "const f: any = pick(Object.getOwnPropertyDescriptors); const d = f({ k: o }); console.log(d);",
    "const f: any = pick(Object.fromEntries); const d = f([['k', o]]); console.log(d);",
    "const f: any = pick(Object.groupBy); const d = f([o], (x: any) => 'a'); console.log(d);",
];

/// The direct call's answer where the direct model itself hands a value to
/// unknown code: a descriptor's get / set are ⊤-called (`own_descriptor`), so
/// getOwnPropertyDescriptor(s) of an object holding `o` holds the realm in
/// both forms. Every other row: neither held nor escaped.
fn direct_answer(b: &str) -> (bool, bool) {
    if b.contains("getOwnPropertyDescriptors") {
        run("const d = Object.getOwnPropertyDescriptors({ k: o }); console.log(d);")
    } else if b.contains("getOwnPropertyDescriptor") {
        run("const d = Object.getOwnPropertyDescriptor({ k: o }, 'k'); console.log(d);")
    } else {
        (false, false)
    }
}

/// (i) Each of the eight called as a value: the realm is not held, `o` does
/// not escape.
#[test]
fn a_value_call_of_the_eight_runs_no_unknown_code() {
    let bad: Vec<String> = VALUE_CALLS
        .iter()
        .map(|b| (b, run(b)))
        .filter(|(b, r)| *r != direct_answer(b))
        .map(|(b, r)| format!("{r:?} {b}"))
        .collect();
    assert!(bad.is_empty(), "held/escaped:\n{}", bad.join("\n"));
}

/// (ii) The skin shape: one field holds a program method and Object.create.
#[test]
fn skin_shape_field_with_method_and_create() {
    assert_eq!(
        run("const x: any = {}; x.m = process.argv.length > 9 ? function (_a: any) { return 1; } : pick(Object.create); console.log(x.m(o));"),
        (false, false)
    );
}

/// (iii) Probe (not a gate): each Object static read under an unknown key,
/// called as a value: which still hold the realm.
#[test]
fn valcall_probe_object_statics() {
    for n in [
        "assign", "create", "defineProperties", "defineProperty", "entries", "freeze",
        "fromEntries", "getOwnPropertyDescriptor", "getOwnPropertyDescriptors",
        "getOwnPropertyNames", "getOwnPropertySymbols", "getPrototypeOf", "groupBy", "hasOwn",
        "is", "isExtensible", "isFrozen", "isSealed", "keys", "preventExtensions", "seal",
        "setPrototypeOf", "values",
    ] {
        let b = format!("const f: any = pick((Object as any).{n}); try {{ f(o, o); }} catch (e) {{}}");
        eprintln!("VALCALL {n} (held,escapes)={:?}", run(&b));
    }
    eprintln!(
        "VALCALL [argv] (held,escapes)={:?}",
        run("const kk = process.argv[3]; const f: any = (Object as any)[kk]; try { f(o, o); } catch (e) {}")
    );
}

/// (iv) Semantics of the direct call kept: the prototype given to create /
/// setPrototypeOf answers a method call; a getter installed by defineProperty
/// is called on read; groupBy calls its callback with the items.
#[test]
fn value_calls_keep_direct_semantics() {
    let bad: Vec<&str> = [
        "const P: any = { pm() { return new Svc(); } }; const f: any = pick(Object.create); const c: any = f(P); console.log(c.pm().marker);",
        "const P: any = { pm() { return new Svc(); } }; const f: any = pick(Object.setPrototypeOf); const c: any = {}; f(c, P); console.log(c.pm().marker);",
        "const f: any = pick(Object.defineProperty); const t: any = {}; f(t, 'k', { get() { return new Svc(); } }); console.log(t.k.marker);",
        "const f: any = pick(Object.defineProperty); const t: any = {}; f(t, 'k', { value: new Svc() }); console.log(t.k.marker);",
        "const f: any = pick(Object.defineProperties); const t: any = {}; f(t, { k: { value: new Svc() } }); console.log(t.k.marker);",
        "const f: any = pick(Object.groupBy); f([new Svc()], (x: any) => { console.log(x.marker); return 'a'; });",
    ]
    .into_iter()
    .filter(|b| !proven(b, "marker"))
    .collect();
    assert!(bad.is_empty(), "not proven:\n{}", bad.join("\n"));
}

/// (v) Where the direct model proves less, the value call answers as it.
#[test]
fn value_calls_answer_as_the_direct_call() {
    for (v, d) in [
        (
            "const f: any = pick(Object.getOwnPropertyDescriptor); const d: any = f({ k: new Svc() }, 'k'); console.log(d.value.marker);",
            "const d: any = Object.getOwnPropertyDescriptor({ k: new Svc() }, 'k'); console.log(d.value.marker);",
        ),
        (
            "const f: any = pick(Object.fromEntries); const d: any = f([['k', new Svc()]]); console.log(d.k.marker);",
            "const d: any = Object.fromEntries([['k', new Svc()]]); console.log(d.k.marker);",
        ),
    ] {
        assert_eq!(proven(v, "marker"), proven(d, "marker"), "{v}");
    }
}

/// Controls: a host function value and a ⊤ callee still hold the realm and
/// escape `o`; Object.create with a ⊤ prototype answers as the direct call.
#[test]
fn controls_keep_todays_answer() {
    assert_eq!(run("(globalThis as any).hostTake(o);"), (true, true));
    assert_eq!(
        run("const hf: any = (globalThis as any)[process.argv[3]]; hf(o);"),
        (true, true)
    );
    let top = "const tp: any = JSON.parse(process.argv[4] ?? 'null');";
    assert_eq!(
        run(&format!("{top} const c: any = Object.create(tp); console.log(c.zz);")),
        run(&format!("{top} const f: any = pick(Object.create); const c: any = f(tp); console.log(c.zz);"))
    );
}

/// Probe (not a gate): the direct calls beside the value calls.
#[test]
fn valcall_probe_direct() {
    for b in [
        "const d = Object.getOwnPropertyDescriptor({ k: o }, 'k'); console.log(d);",
        "const d = Object.getOwnPropertyDescriptors({ k: o }); console.log(d);",
        "const d = Object.fromEntries([['k', o]]); console.log(d);",
    ] {
        eprintln!("VALCALLD (held,escapes)={:?} {b}", run(b));
    }
    for b in [
        "const d: any = Object.getOwnPropertyDescriptor({ k: new Svc() }, 'k'); console.log(d.value.marker);",
        "const d: any = Object.fromEntries([['k', new Svc()]]); console.log(d.k.marker);",
    ] {
        eprintln!("VALCALLD proven={} {b}", proven(b, "marker"));
    }
}

/// Probe (not a gate): the unknown-key call with the diagnostic on.
#[test]
fn valcall_probe_unksrc() {
    std::env::set_var("PERRY_WPF_UNKSRC", "1");
    eprintln!(
        "VALCALLD [argv] {:?}",
        run("const kk = process.argv[3]; const f: any = (Object as any)[kk]; try { f(o, o); } catch (e) {}")
    );
    std::env::remove_var("PERRY_WPF_UNKSRC");
}
