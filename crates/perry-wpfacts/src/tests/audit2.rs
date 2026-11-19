//! AUDIT2 (2026-11-15): fixtures of the second independent solver audit.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

/// The verdict names of the sites of key `key`, in source order.
fn names(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    s.iter().map(|s| format!("{:?}", s.verdict)).collect()
}

#[test]
fn audit2_trivial() {
    let pf = one("const o = { m() { return 1; } };\nconst r = o.m();\n");
    assert!(!names(&pf, "m", true).is_empty());
}

const P: &str = "function g(): any { return 2; }\nconst f: any = { a() { return 1; } };\nfunction cb(this: any, ...r: any[]): any { f.a = g; return 0; }\nconst o: any = { toString() { f.a = g; return \"1\"; }, valueOf() { f.a = g; return 1; } };\n";

fn v(src: &str, key: &str, call: bool) -> Vec<String> {
    let pf = one(&format!("{P}{src}"));
    let v = names(&pf, key, call);
    eprintln!("AUDIT2 {key} call={call}: {v:?}  <= {src}");
    assert!(!v.is_empty(), "no site of {key}");
    v
}
fn has(v: &[String], want: &str) -> bool {
    v.iter()
        .any(|x| x == want || x.starts_with(&format!("{want}(")))
}
/// `f.a()` after `src`: JavaScript has run `f.a = g` (through `cb` or a conversion of `o`), so
/// the call may reach g: T0Mono (one callee, the literal method) is wrong.
fn reaches_g(src: &str) -> bool {
    !has(&v(&format!("{src}\nf.a();\n"), "a", true), "T0Mono")
}
/// The members of `srcs` after which the solver still proves `f.a()` calls the literal method.
fn missed<'a>(srcs: &[&'a str]) -> Vec<&'a str> {
    srcs.iter().copied().filter(|s| !reaches_g(s)).collect()
}

// ---- UNSOUND: each test states what JavaScript does; red = the solver proves the opposite.

/// A1: arguments that reach a builtin through a spread or through `apply` / `Reflect.apply` are
/// its arguments (13.3.8.1 ArgumentListEvaluation, 20.2.3.1 Function.prototype.apply, 28.1.1).
#[test]
fn audit2_u1_spread_callback_into_builtin() {
    let bad = missed(&[
        "[1].forEach(...[cb]);",
        "Array.prototype.forEach.apply([1], [cb]);",
        "Reflect.apply(Array.prototype.forEach, [1], [cb]);",
        "Promise.resolve(1).then(...([cb] as [any]));",
        "new Map([[1, 1]]).forEach(...([cb] as [any]));",
        "Array.from(...([[1], cb] as [any, any]));",
        "JSON.parse(...([\"1\", cb] as [any, any]));",
        "process.nextTick(...([cb] as [any]));",
    ]);
    assert!(bad.is_empty(), "cb runs in JS: {bad:?}");
}
/// A1: `Object.assign` with its target inside the spread / the apply array copies all the same
/// (20.1.2.1); `[].concat.apply([], parts)` holds the elements of the parts (23.1.3.2).
#[test]
fn audit2_u1b_spread_object_assign_and_concat() {
    let bad = missed(&[
        "Object.assign(...([f, { a: g }] as [any, any]));",
        "Object.assign.apply(null, [f, { a: g }]);",
        "Reflect.apply(Object.assign, null, [f, { a: g }]);",
        "Array.prototype.concat.apply([], [[g]]).forEach((x: any) => { f.a = x; });",
    ]);
    assert!(bad.is_empty(), "f.a is g in JS: {bad:?}");
}
/// A1: a builtin called with a spread reads every position from the spread (13.3.8.1): the value
/// at spread position 5 lands at position 5 and later (23.1.3.31 splice items, 23.1.2.3
/// Array.of, 23.1.3.35 toSpliced items, 20.2.3.3 Function.prototype.call into parameter 6).
#[test]
fn audit2_u1f_spread_late_position() {
    let bad = missed(&[
        "const xs: any[] = []; xs.splice(0, 0, ...([1, 2, 3, 4, 5, g] as any[])); xs.forEach((x: any) => { f.a = x; });",
        "Array.of(...([1, 2, 3, 4, 5, g] as any[])).forEach((x: any) => { f.a = x; });",
        "[0].toSpliced(0, 0, ...([1, 2, 3, 4, 5, g] as any[])).forEach((x: any) => { f.a = x; });",
        "function h(a: any, b: any, c: any, d: any, e: any, k: any) { f.a = k; } h.call(null, ...([1, 2, 3, 4, 5, g] as any[]));",
    ]);
    assert!(bad.is_empty(), "f.a is g in JS: {bad:?}");
}
/// A1 control: the same without a spread is seen (green); a call that converts nothing keeps
/// T0Mono, so `reaches_g` can fail.
#[test]
fn audit2_u1_control_direct() {
    assert!(reaches_g("[1].forEach(cb);"));
    assert!(reaches_g("Object.assign(f, ...[{ a: g }]);"));
    assert!(
        !reaches_g("[1].includes(o);"),
        "SameValueZero converts nothing"
    );
}

/// A2: `delete ob[o]` runs ToPropertyKey(o) (13.5.1.2 step 5.b, 7.1.19): `o.toString()` is called.
#[test]
fn audit2_u2_delete_key_to_property_key() {
    assert!(
        reaches_g("const ob: any = {}; delete ob[o];"),
        "toString runs in JS"
    );
}
/// A2: `Object.fromEntries` creates each property under ToPropertyKey(key) (20.1.2.7, 7.3.6).
#[test]
fn audit2_u2b_from_entries_key_to_property_key() {
    assert!(
        reaches_g("Object.fromEntries([[o, 1]]);"),
        "toString runs in JS"
    );
}
/// A2: the index arguments of these Array methods are ToIntegerOrInfinity (23.1.3.31 splice,
/// 23.1.3.20 lastIndexOf, 23.1.3.4 copyWithin, 23.1.3.39 with, 23.1.3.35 toSpliced): `valueOf` runs.
#[test]
fn audit2_u2c_array_index_argument_to_number() {
    let bad = missed(&[
        "[1].splice(o);",
        "[1].lastIndexOf(1, o);",
        "[1].copyWithin(o, 0);",
        "[1].with(o, 2);",
        "[1].toSpliced(o);",
    ]);
    assert!(bad.is_empty(), "valueOf runs in JS: {bad:?}");
}
/// A2 control: the fromIndex of `indexOf` and a computed read key are converted (green).
#[test]
fn audit2_u2_control_index_of() {
    assert!(reaches_g("[1].indexOf(1, o);"));
    assert!(reaches_g("const ob: any = {}; ob[o];"));
}

/// A3: for-in visits the enumerable String keys of the prototype chain (14.7.5.9
/// EnumerateObjectProperties); the `__proto__: p` member of a literal sets that chain (13.2.5.5).
#[test]
fn audit2_u3_for_in_literal_proto_keys() {
    let bad = missed(&[
        "const src: any = { __proto__: { a: 1 } }; for (const k in src) f[k] = g;",
        "const p = { a: 1 }; const src: any = { __proto__: p, b: 2 }; for (const k in src) f[k] = g;",
    ]);
    assert!(bad.is_empty(), "k is \"a\" in JS: {bad:?}");
}
/// A3: the same loop fills `h.a`; the read `h.a.zz` does not throw.
#[test]
fn audit2_u3b_for_in_literal_proto_throws() {
    let r = v("const src: any = { __proto__: { a: 1 } }; const h: any = {}; for (const k in src) h[k] = { zz: 1 }; h.a.zz;\n", "zz", false);
    assert!(
        !has(&r, "Throws") && !has(&r, "Dead"),
        "h.a is an object in JS: {r:?}"
    );
}
/// A3 control: the same chain made by `Object.create` is enumerated (green).
#[test]
fn audit2_u3_control_object_create() {
    assert!(reaches_g(
        "const src: any = Object.create({ a: 1 }); for (const k in src) f[k] = g;"
    ));
}

/// A4: unknown code that holds a promise reads its fulfilment value (27.2.5.4 then): the value
/// escapes with the promise, whoever made the promise.
#[test]
fn audit2_u4_promise_value_escapes_with_promise() {
    let bad = missed(&[
        "declare function ext(x: any): any;\nasync function h() { return f; } ext(h());",
        "declare function ext(x: any): any;\next((async () => f)());",
        "declare function ext(x: any): any;\next(new Promise((r) => r(f)));",
        "declare function ext(x: any): any;\nasync function h() { throw f; } ext(h());",
    ]);
    assert!(bad.is_empty(), "unknown code may store f.a: {bad:?}");
}
/// A4 control: `Promise.resolve(f)` escapes f (green).
#[test]
fn audit2_u4_control_promise_resolve() {
    assert!(reaches_g(
        "declare function ext(x: any): any;\next(Promise.resolve(f));"
    ));
}

/// A5: `a.__proto__ = null` gives `a` no prototype (B.2.2.1.2, 10.1.2.1): `a.m` is undefined and
/// the call throws a TypeError; T0Mono names A::m as the one callee.
#[test]
fn audit2_u5_null_proto_store() {
    let mut bad = Vec::new();
    for s in [
        "class A { m() { return 1; } }\nconst a: any = new A(); a.__proto__ = null; a.m();\n",
        "class A { m() { return 1; } } class B extends A {}\n(B.prototype as any).__proto__ = null; new B().m();\n",
        "class A { m() { return 1; } }\nfunction setp(x: any, p: any) { x.__proto__ = p; } const a: any = new A(); setp(a, null); a.m();\n",
    ] {
        if has(&v(s, "m", true), "T0Mono") {
            bad.push(s);
        }
    }
    assert!(bad.is_empty(), "a.m() throws in JS: {bad:?}");
}
/// A5 control: a relink to an object without `m` is refused (green).
#[test]
fn audit2_u5_control_relink_object() {
    let r = v(
        "class A { m() { return 1; } }\nconst a: any = new A(); a.__proto__ = { n: 1 }; a.m();\n",
        "m",
        true,
    );
    assert!(has(&r, "No"), "{r:?}");
}

/// A6: the own `lastIndex` of a RegExp is a data property holding a Number (22.2.3.1
/// RegExpAlloc): `.value` is 0 and `(0).zz` is undefined, no throw.
#[test]
fn audit2_u6_regexp_last_index_descriptor() {
    let r = v(
        "const d: any = Object.getOwnPropertyDescriptor(/a/g, \"lastIndex\")!.value; d.zz;\n",
        "zz",
        false,
    );
    assert!(!has(&r, "Throws") && !has(&r, "Dead"), "{r:?}");
}

/// A1: a class that extends a builtin and forwards `...args` to `super` passes them on
/// (10.2.2 [[Construct]], 13.3.7.1 SuperCall); a bound `apply` is `apply` (10.4.1.1).
#[test]
fn audit2_u1d_spread_into_builtin_super_and_bound_apply() {
    let bad = missed(&[
        "class MyP extends Promise<any> { constructor(...a: any[]) { super(...(a as [any])); } } new MyP(cb);",
        "const applyb = Function.prototype.apply.bind(Array.prototype.forEach); applyb([1], [cb]);",
    ]);
    assert!(bad.is_empty(), "cb runs in JS: {bad:?}");
}
/// A1: the solver lists no site in the callback body (the body counts as never run); with the
/// callback passed directly the site exists (the control half, green).
#[test]
fn audit2_u1e_spread_callback_body_has_no_site() {
    let direct = one(&format!(
        "{P}[{{ zz: 1 }}].forEach((x: any) => {{ x.zz; }});\n"
    ));
    assert!(
        !names(&direct, "zz", false).is_empty(),
        "control: direct callback has its site"
    );
    let spread = one(&format!(
        "{P}[{{ zz: 1 }}].forEach(...[(x: any) => {{ x.zz; }}]);\n"
    ));
    let r = names(&spread, "zz", false);
    eprintln!("AUDIT2 zz spread: {r:?}");
    assert!(
        !r.is_empty() && !has(&r, "Dead"),
        "x.zz is reached in JS; the solver has no live site: {r:?}"
    );
}
/// A2: more index arguments that are ToIntegerOrInfinity (23.1.3.28 slice, 23.1.3.1 at,
/// 23.1.3.13 flat depth, splice deleteCount, copyWithin start / end, toSpliced skipCount).
#[test]
fn audit2_u2d_array_index_argument_to_number_more() {
    let bad = missed(&[
        "[1, 2].slice(o);",
        "[1, 2].slice(0, o);",
        "[1, 2].at(o);",
        "[1, 2].flat(o);",
        "[1, 2].splice(0, o);",
        "[1, 2].copyWithin(0, o);",
        "[1, 2].copyWithin(0, 0, o);",
        "[1, 2].toSpliced(0, o);",
    ]);
    assert!(bad.is_empty(), "valueOf runs in JS: {bad:?}");
}
/// A4: more ways a promise with a program value reaches unknown code.
#[test]
fn audit2_u4b_promise_value_escapes_more() {
    let d = "declare function ext(x: any): any;\nasync function hq() { return f; }\n";
    let bad: Vec<String> = [
        "ext(async () => f);",
        "ext(() => hq());",
        "ext(hq().then((x) => x));",
        "ext([hq()]);",
        "ext(new Promise((_r, rej) => rej(f)));",
        "let rr: any; const pw = new Promise((r) => { rr = r; }); ext(pw); rr(f);",
    ]
    .into_iter()
    .map(|s| format!("{d}{s}"))
    .filter(|s| !reaches_g(s))
    .collect();
    assert!(bad.is_empty(), "unknown code may store f.a: {bad:?}");
}

// ---- IMPRECISE: each test states the narrow exact answer; red = the solver gives a wide one.

fn proven(src: &str, key: &str) -> bool {
    let r = v(src, key, true);
    r.iter().all(|x| x == "T0Mono" || x == "T0Set")
}

/// J1: a key that is a Symbol and nothing else names no String-keyed property and is never
/// `__proto__` (7.1.19 ToPropertyKey, 6.1.7): `x[sy] = v` leaves `x.m` and the prototype alone.
#[test]
fn audit2_i1_symbol_key_store() {
    let r = v("const sy = Symbol(); class A { m() { return 1; } }\nconst x: any = new A(); x[sy] = { m: g }; x.m();\n", "m", true);
    assert!(has(&r, "T0Mono"), "x.m() is A::m: {r:?}");
}
/// J1: the read `x[sy]` yields the value stored under the symbol or undefined, no String-keyed
/// member of the chain.
#[test]
fn audit2_i1b_symbol_key_read() {
    assert!(
        proven(
            "const sy = Symbol(); const x: any = {}; x[sy] = f; x[sy].a();\n",
            "a"
        ),
        "x[sy] is f"
    );
}
/// J2: a computed store whose key shape cannot spell `m` (a known prefix, a Number: 7.1.19,
/// 6.1.6.1.20 Number::toString) leaves the prototype method call proven.
#[test]
fn audit2_i2_shaped_key_store_on_instance() {
    for s in [
        "class A { m() { return 1; } }\nconst x: any = new A(); const u: string = JSON.parse('\"x\"'); x[\"k_\" + u] = 1; x.m();\n",
        "class A { m() { return 1; } }\nconst x: any = new A(); const n: number = Date.now(); x[n] = 1; x.m();\n",
        "class A { m() { return 1; } }\nconst x: any = new A(); const n: number = Date.now(); x[String(n)] = 1; x.m();\n",
    ] {
        assert!(proven(s, "m"), "the key never spells m");
    }
}
/// J2 control: a constant other key keeps the call (green).
#[test]
fn audit2_i2_control_constant_key() {
    assert!(proven("class A { m() { return 1; } }\nconst x: any = new A(); const k: string = [\"extra\"][0]; x[k] = 1; x.m();\n", "m"));
}
/// J3: `concat` spreads an Array argument (23.1.3.2 step 5.b IsConcatSpreadable): the argument
/// array is never an element of the result.
#[test]
fn audit2_i3_concat_array_argument() {
    assert!(
        proven("const xs: any[] = [f]; xs.concat([])[0].a();\n", "a"),
        "elements are f only"
    );
}
/// J4: `flat` flattens Array elements only (23.1.3.13.1 FlattenIntoArray step 3.c.v IsArray):
/// an object element stays itself, its fields are no elements.
#[test]
fn audit2_i4_flat_object_element() {
    let r = v(
        "const xs: any[] = [{ a() { return 1; }, 0: { a: g } }]; xs.flat()[0].a();\n",
        "a",
        true,
    );
    assert!(
        r.iter().all(|x| x == "T0Mono"),
        "the one element is the object, its field 0 is no element: {r:?}"
    );
}
/// J5: `delete x[k]` removes one own property (10.1.10.1); a prototype method of a class
/// instance is untouched.
#[test]
fn audit2_i5_delete_on_instance() {
    assert!(
        proven(
            "class A { m() { return 1; } }\nconst x: any = new A(); x.q = 1; delete x.q; x.m();\n",
            "m"
        ),
        "m is on the prototype"
    );
}
/// J6: a catch binding holds what its try block throws (14.15.3); `throw f` alone gives f.
#[test]
fn audit2_i6_catch_binding() {
    assert!(
        proven("try { throw f; } catch (e: any) { e.a(); }\n", "a"),
        "e is f"
    );
}

/// The `a` call verdicts of `src` (two receivers h1, h2 born as `{ a() { return 1; } }`).
fn two(src: &str) -> Vec<String> {
    v(&format!("const h1: any = {{ a() {{ return 1; }} }}; const h2: any = {{ a() {{ return 1; }} }};\n{src}\nh1.a(); h2.a();\n"), "a", true)
}
/// A7: one closure that stores under its captured key into its argument, called with two
/// objects: each call stores into its own argument (13.15.2 AssignmentExpression, 10.2.1.1: one
/// evaluation per call). The solver stores into the first receiver only: `h2.a()` stays T0Mono.
#[test]
fn audit2_u7_captured_key_store_two_receivers() {
    let mut bad = Vec::new();
    for s in [
        "function mk(K: string) { return (x: any) => { x[K] = g; }; } const st = mk([\"a\"][0]); st(h1); st(h2);",
        "function mk(K: string) { return (x: any) => { x[K] = g; }; } const st = mk(\"a\"); st(h1); st(h2);",
        "function mk(K: string) { return function (x: any) { x[K] = g; }; } const st = mk([\"a\", \"b\"][0]); st(h1); st(h2);",
        "const mk = (K: string) => (x: any, w: any) => { x[K] = w; }; const st = mk([\"a\"][0]); st(h1, g); st(h2, g);",
    ] {
        let r = two(s);
        if has(&r, "T0Mono") {
            bad.push(format!("{r:?} <= {s}"));
        }
    }
    assert!(bad.is_empty(), "both h1.a and h2.a are g in JS: {bad:#?}");
}
/// A7 control: the same with a module-level key is seen for both receivers (green).
#[test]
fn audit2_u7_control_module_key() {
    let r = two("let K: string = [\"a\"][0]; function st(x: any) { x[K] = g; } st(h1); st(h2);");
    assert!(!has(&r, "T0Mono"), "{r:?}");
}
