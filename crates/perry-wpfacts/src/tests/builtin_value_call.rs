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

/// SKINHOLD2 (three.js skin, AnimationAction::_update
/// `propertyMixers[ j ].accumulate( accuIndex, weight )`): one call whose
/// callee set holds `Object.create` and a callee returning the `Object`
/// constructor. `Object.create(p, props)` defines `props` only on the object
/// it creates (ECMA-262 20.1.2.2 steps 2-3, ObjectDefineProperties on `obj`),
/// never on another callee's result: `Object` is not written.
#[test]
fn red_skinhold2_create_props_define_on_the_created_object() {
    assert_eq!(
        run("const f: any = process.argv.length > 9 ? function (_p: any, _d: any) { return Object; } : pick(Object.create); const c: any = f(null, { a: { value: 1 } }); console.log(c);"),
        (false, false)
    );
}

/// Control: a defineProperty under an unknown key on a value that may be
/// the `Object` constructor really writes it: the write stays.
#[test]
fn skinhold2_control_define_on_object_constructor_stays() {
    assert!(
        run("const t: any = process.argv.length > 9 ? Object : {}; Object.defineProperty(t, process.argv[3] ?? \"q\", { value: 1 }); console.log(t);").0,
        "a define under an unknown key on a value holding Object writes Object"
    );
}

/// SKINHOLD3: three.js skin `propertyMixers[ j ].accumulate( accuIndex,
/// weight )` (AnimationAction::_update): the receiver may be `Object` and a
/// primitive (`bindingsByName[ trackName ]` on `Object`, reached through
/// `bindingsByRoot[ rootUuid ]` = `Object.prototype.constructor`, reads its
/// `length` / `name`), the callee `Object.prototype.__lookupGetter__`
/// (stored onto `Object` by `bindingByName[ trackName ] = binding` in
/// `_addInactiveBinding`). B.2.2.4 on a primitive `this` runs on its
/// wrapper (7.1.18): own data properties or none, then its kind's
/// prototype chain: no unknown code, the realm is not held. The store's
/// key is named: an unknown key may be `__proto__` (B.2.2.1.2), which
/// gives `Object` a prototype whose chain has an accessor (E6, a separate
/// hold, SKINHOLD4 note).
#[test]
fn red_skinhold3_lookup_getter_on_a_primitive_walks_its_prototype() {
    assert_eq!(
        run("const r: any = process.argv.length > 9 ? Object : 1; (Object as any).accumulate = ({} as any)[json.j]; const h: any = r.accumulate(1, 0.5); console.log(h(o));"),
        (false, false)
    );
}

/// Control: the direct call on a primitive answers the same way.
#[test]
fn skinhold3_control_direct_lookup_getter_on_a_number() {
    assert_eq!(
        run("const h: any = (1 as any).__lookupGetter__(json.k); console.log(h(o));"),
        (false, false)
    );
}

/// Control: a program getter installed on `Object.prototype` under an
/// unknown key is what the lookup on a primitive may return: it runs, and
/// its result is the program value it returns.
#[test]
fn skinhold3_control_program_getter_on_the_chain_is_found() {
    assert!(
        !proven("const json: any = JSON.parse(process.argv[2] ?? \"{}\"); (Object.prototype as any).__defineGetter__(json.k, function () { return json; }); const h: any = (1 as any).__lookupGetter__(json.k); const s: any = h(); console.log(s.marker);", "marker"),
        "the getter returns a JSON value: s.marker reads an unknown object"
    );
}

/// SKINHOLD4 control: a String wrapper's `length` is an own data
/// property (10.4.3), so B.2.2.4/5 on a string answers `undefined` for it:
/// no unknown function.
#[test]
fn skinhold4_control_lookup_on_a_string_own_data_is_undefined() {
    assert_eq!(
        run("const h: any = (\"abc\" as any).__lookupGetter__(\"length\"); const k: any = (\"abc\" as any).__lookupSetter__(\"length\"); if (h !== undefined) h(o); if (k !== undefined) k(o);"),
        (false, false)
    );
}

/// SKINHOLD4 control: on a number the walk from Number.prototype reaches
/// Object.prototype's realm `__proto__` accessor (B.2.2.1): its getter
/// reads the receiver's [[Prototype]] and runs no unknown code.
#[test]
fn skinhold4_control_lookup_proto_getter_on_a_number_is_the_realm_accessor() {
    assert_eq!(
        run("const h: any = (5 as any).__lookupGetter__(\"__proto__\"); const p: any = h.call(o); console.log(p);"),
        (false, false)
    );
}
