//! AUDIT1 (2026-11-15): fixtures of the independent solver audit.
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
fn audit1_trivial() {
    let pf = one("const o = { m() { return 1; } };\nconst r = o.m();\n");
    assert!(!names(&pf, "m", true).is_empty());
}

const G: &str = "function g(): any { return 2; }\n";

fn v(src: &str, key: &str, call: bool) -> Vec<String> {
    let pf = one(&format!("{G}{src}"));
    let v = names(&pf, key, call);
    eprintln!("AUDIT1 {key} call={call}: {v:?}  <= {src}");
    assert!(!v.is_empty(), "no site of {key}");
    v
}
fn has(v: &[String], want: &str) -> bool {
    v.iter()
        .any(|x| x == want || x.starts_with(&format!("{want}(")))
}
fn last_is(v: &[String], want: &str) -> bool {
    has(&v[v.len() - 1..], want)
}

// ---- UNSOUND: each test states what JavaScript does; red = the solver proves the opposite.

/// U1: `s[0]` of a primitive String is a String (10.4.3.5 StringGetOwnProperty); the store
/// `f["a"] = g` makes `f.a()` call g. The solver gives `s[0]` no value: the store is lost.
#[test]
fn audit1_u1_string_index_key_store() {
    let r = v("const f = { a() { return 1; } };\nconst s: string = \"ab\";\n(f as any)[s[0]] = g;\nf.a();\n", "a", true);
    assert!(!has(&r, "T0Mono"), "f.a() calls g in JS: {r:?}");
}
/// U1 control: the same through `charAt` is seen (green).
#[test]
fn audit1_u1_control_charat() {
    let r = v("const f = { a() { return 1; } };\nconst s: string = \"ab\";\n(f as any)[s.charAt(0)] = g;\nf.a();\n", "a", true);
    assert!(!has(&r, "T0Mono"), "{r:?}");
}
/// U1b: `c.zz` runs (c is \"a\"): not Dead.
#[test]
fn audit1_u1b_string_index_dead() {
    let r = v(
        "function f(s: string, i: number) { const c: any = s[i]; return c.zz; }\nf(\"abc\", 1);\n",
        "zz",
        false,
    );
    assert!(!has(&r, "Dead"), "c.zz is reached in JS: {r:?}");
}
/// U1c: a Number receiver under a computed key reads Number.prototype (7.3.2 GetV): `h` is toFixed.
#[test]
fn audit1_u1c_number_computed_key_dead() {
    let r = v(
        "const u: any = 5;\nconst j: string = [\"toFixed\"][0];\nconst h = u[j];\nh.zz;\n",
        "zz",
        false,
    );
    assert!(
        !has(&r, "Dead"),
        "h is Number.prototype.toFixed in JS: {r:?}"
    );
}
/// U1 KEEP: a character of a String is no function and stores nothing: `f.a()` keeps its one callee.
#[test]
fn audit1_u1_keep_string_index_read() {
    let r = v("const f = { a() { return 1; } };\nconst s: string = \"ab\";\nconst c: any = s[0];\nc.length;\nf.a();\n", "a", true);
    assert!(has(&r, "T0Mono"), "f.a() calls its own method only: {r:?}");
}
/// U1 KEEP: an array element read under a String's index key stays exact: `o.m()` keeps its callee.
#[test]
fn audit1_u1_keep_array_by_char_key() {
    let r = v("const o = { m() { return 1; } };\nconst xs: any = { a: o };\nconst s: string = \"ab\";\nxs[s[0]].m();\no.m();\n", "m", true);
    assert!(
        last_is(&r, "T0Mono"),
        "o.m() calls its own method only: {r:?}"
    );
}
/// U2: a `__proto__` store on a class instance relinks it (B.2.2.1.2): `a.m()` calls g.
#[test]
fn audit1_u2_proto_store_class_instance() {
    let r = v(
        "class A { m() { return 1; } }\nconst a: any = new A();\na.__proto__ = { m: g };\na.m();\n",
        "m",
        true,
    );
    assert!(!has(&r, "T0Mono"), "a.m() calls g in JS: {r:?}");
}
/// U2b: the same on a class prototype: `new B().m()` calls g.
#[test]
fn audit1_u2b_proto_store_class_prototype() {
    let r = v("class A { m() { return 1; } }\nclass B extends A {}\n(B.prototype as any).__proto__ = { m: g };\nnew B().m();\n", "m", true);
    assert!(!has(&r, "T0Mono"), "new B().m() calls g in JS: {r:?}");
}
/// U2c: `Reflect.set(a, \"__proto__\", v)` runs the same setter (10.1.9.2 step 6, B.2.2.1.2).
#[test]
fn audit1_u2c_reflect_set_proto() {
    let r = v("class A { m() { return 1; } }\nconst a = new A();\nReflect.set(a, \"__proto__\", { m: g });\na.m();\n", "m", true);
    assert!(!has(&r, "T0Mono"), "a.m() calls g in JS: {r:?}");
}
/// U3: `Reflect.get(target, key, receiver)` runs the getter with this = receiver (28.1.5, 10.1.8.1 step 7).
#[test]
fn audit1_u3_reflect_get_receiver() {
    let r = v("class P { m() { return 1; } get gg(): any { return this.m(); } }\nReflect.get(P.prototype, \"gg\", { m: g });\n", "m", true);
    assert!(!has(&r, "T0Mono"), "this.m() calls g in JS: {r:?}");
}
/// U4: `Reflect.set(target, key, v, receiver)` runs the setter with this = receiver (10.1.9.2 step 7).
#[test]
fn audit1_u4_reflect_set_receiver() {
    let r = v("class P { m() { return 1; } set ss(x: any) { this.m(); } }\nReflect.set(P.prototype, \"ss\", 1, { m: g });\n", "m", true);
    assert!(!has(&r, "T0Mono"), "this.m() calls g in JS: {r:?}");
}
/// U5: the descriptor of `Object.prototype.__proto__` is an accessor descriptor with both functions
/// (B.2.2.1): `set.call(...)` does not throw, and relinks `a`.
#[test]
fn audit1_u5_builtin_accessor_descriptor() {
    let src = "class A { m() { return 1; } }\nconst a = new A();\nconst set = (Object.getOwnPropertyDescriptor(Object.prototype, \"__proto__\") as any).set;\nset.call(a, { m: g });\na.m();\n";
    let c = v(src, "call", true);
    let m = v(src, "m", true);
    assert!(
        !has(&c, "Throws") && !has(&m, "T0Mono"),
        "set is a function, a.m() calls g: {c:?} {m:?}"
    );
}
/// U5b: own data members of a function and of a namespace have data descriptors with their value
/// (20.2.4.2 name, 21.3.1.6 Math.PI): `.value.zz` is `undefined`, no TypeError.
#[test]
fn audit1_u5b_intrinsic_data_descriptor() {
    let a = v(
        "const X: any = (Object.getOwnPropertyDescriptor(g, \"name\") as any).value;\nX.zz;\n",
        "zz",
        false,
    );
    let b = v(
        "const X: any = (Object.getOwnPropertyDescriptor(Math, \"PI\") as any).value;\nX.zz;\n",
        "zz",
        false,
    );
    let c = v(
        "const X: any = (Object.getOwnPropertyDescriptor(g, \"prototype\") as any).value;\nX.zz;\n",
        "zz",
        false,
    );
    assert!(
        !has(&a, "Throws") && !has(&b, "Throws") && !has(&c, "Throws"),
        "{a:?} {b:?} {c:?}"
    );
}
/// U6: a String source has own enumerable index keys (10.4.3): `Object.assign({}, \"ab\")[0]` is \"a\".
#[test]
fn audit1_u6_string_source_index_keys() {
    let a = v("const f = { a() { return 1; } };\nconst t: any = Object.assign({}, \"ab\");\n(f as any)[t[0]] = g;\nf.a();\n", "a", true);
    let b = v("const f = { a() { return 1; } };\nconst t: any = { ...(\"ab\" as any) };\n(f as any)[t[0]] = g;\nf.a();\n", "a", true);
    let c = v("const f = { a() { return 1; } };\nconst t: any = new String(\"ab\");\n(f as any)[t[0]] = g;\nf.a();\n", "a", true);
    assert!(
        !has(&a, "T0Mono") && !has(&b, "T0Mono") && !has(&c, "T0Mono"),
        "f.a() calls g: {a:?} {b:?} {c:?}"
    );
}
/// U6 KEEP: an object source copies its own keys only: `t.a` is the method, `f.a()` keeps its callee.
#[test]
fn audit1_u6_keep_object_source() {
    let r = v("const f = { a() { return 1; } };\nconst t: any = Object.assign({}, { b: 1 });\nconst c = t.b;\nf.a();\n", "a", true);
    assert!(has(&r, "T0Mono"), "f.a() calls its own method only: {r:?}");
}
/// U7: `Object.fromEntries([[\"a\", {q:1}]]).a` is the object (20.1.2.7): `.zz` is `undefined`, no TypeError.
#[test]
fn audit1_u7_from_entries_named_read() {
    let r = v(
        "const X: any = Object.fromEntries([[\"a\", { q: 1 }]]);\nX.a.zz;\n",
        "zz",
        false,
    );
    assert!(!has(&r, "Throws"), "X.a is {{q:1}} in JS: {r:?}");
}

/// U7 / I8: the values of the entries are the values of the result (Map and entries() sources too).
#[test]
fn audit1_u7_from_entries_sources() {
    let a = v("const m = new Map<any, any>([[\"a\", { q: 1 }]]);\nconst X: any = Object.fromEntries(m);\nX.a.zz;\n", "zz", false);
    let b = v("const o = { a: { q: 1 } };\nconst X: any = Object.fromEntries(Object.entries(o));\nX.a.zz;\n", "zz", false);
    assert!(
        !has(&a, "Throws") && !has(&b, "Throws"),
        "X.a is {{q:1}} in JS: {a:?} {b:?}"
    );
}
/// U7 KEEP (I8): a key is no value of the result: the entry key `o` stays unescaped, `o.m()` keeps its callee.
#[test]
fn audit1_u7_keep_key_is_no_value() {
    let r = v("const o = { m() { return 1; } };\nconst X: any = Object.fromEntries([[o, 1]]);\nconst y = X.b;\no.m();\n", "m", true);
    assert!(
        last_is(&r, "T0Mono"),
        "o.m() calls its own method only: {r:?}"
    );
}

// ---- IMPRECISE: red = a wide answer where the exact one is narrow.

/// I1: iterating a Set yields its values only (24.2.5.2): `e` is the literal.
#[test]
fn audit1_i1_set_for_of() {
    let r = v(
        "const s = new Set<any>([{ zz: 1 }]);\nfor (const e of s) { e.zz; }\n",
        "zz",
        false,
    );
    assert!(
        last_is(&r, "T0Mono") || last_is(&r, "T0Set"),
        "e is only the literal: {r:?}"
    );
}
/// I2: `Object.freeze` changes no value and no [[Prototype]] (20.1.2.6, 7.3.16).
#[test]
fn audit1_i2_freeze_keeps_method() {
    let r = v(
        "const o = { m() { return 1; } };\nObject.freeze(o);\no.m();\n",
        "m",
        true,
    );
    assert!(has(&r, "T0Mono"), "{r:?}");
}
/// I3: a constant-key data descriptor defineProperty on another key is one store (10.1.6.3).
#[test]
fn audit1_i3_define_other_key() {
    let r = v("const o = { m() { return 1; } };\nObject.defineProperty(o, \"q\", { value: 1 });\no.m();\n", "m", true);
    assert!(has(&r, "T0Mono"), "{r:?}");
}
/// I4: a truthiness guard on a const removes the nullish arms (13.6.7 / 7.1.2 ToBoolean).
#[test]
fn audit1_i4_truthy_guard_const() {
    let r = v("function mk(): any { return [{ zz: 1 }, null, undefined][g()]; }\nconst x = mk();\nif (x) { x.zz; }\n", "zz", false);
    assert!(has(&r, "T0Mono"), "x is the literal under the guard: {r:?}");
}

/// U8: `Object.assign(B, src)` does `Set(B, \"__proto__\", v)` for a source own key `__proto__`
/// (20.1.2.1 step 3.a.iii.2.b; the key is own on `src` by a computed literal key, 13.2.5.5): the
/// setter relinks class B, `new D()` then runs C as the parent constructor: no field `x`.
#[test]
fn audit1_u8_object_assign_proto_key_relinks_class() {
    let src = "class A { x: any = { zz: 1 }; }\nclass C {}\nclass B extends A {}\nclass D extends B {}\nconst go = () => { const b: any = new D(); b.x.zz; };\nObject.assign(B, { [\"__proto__\"]: C });\ngo();\n";
    let x = v(src, "x", false);
    let z = v(src, "zz", false);
    assert!(
        !has(&x, "T0Mono") && !has(&z, "T0Mono"),
        "b.x is undefined in JS (C constructs): {x:?} {z:?}"
    );
}
/// U8 keep: a source without a key that may be `__proto__` relinks nothing: B keeps A as parent.
#[test]
fn audit1_u8_keep_plain_and_other_computed_key() {
    for a in [
        "Object.assign(B, { a: 1 });",
        "const k = g() ? \"a\" : \"b\";\nObject.assign(B, { [k]: C });",
        "const q: any = { ...{ [\"__proto__\"]: C } };\nq;",
    ] {
        let src = format!("class A {{ x: any = {{ zz: 1 }}; }}\nclass C {{}}\nclass B extends A {{}}\nclass D extends B {{}}\nconst go = () => {{ const b: any = new D(); b.x.zz; }};\n{a}\ngo();\n");
        let x = v(&src, "x", false);
        assert!(has(&x, "T0Mono"), "{a}: {x:?}");
    }
}
/// U8 control: the same relink by `Object.setPrototypeOf` is seen (green).
#[test]
fn audit1_u8_control_set_prototype_of() {
    let src = "class A { x: any = { zz: 1 }; }\nclass C {}\nclass B extends A {}\nclass D extends B {}\nconst go = () => { const b: any = new D(); b.x.zz; };\nObject.setPrototypeOf(B, C);\ngo();\n";
    let x = v(src, "x", false);
    assert!(!has(&x, "T0Mono"), "{x:?}");
}
/// I5: a store on a subclass prototype creates an own property there only (10.1.9.2): instances of
/// the parent and of a sibling class still call A::m alone.
#[test]
fn audit1_i5_subclass_proto_store_widens_parent() {
    let r = v("class A { m(): any { return 1; } }\nclass B extends A {}\nclass C extends A {}\n(B.prototype as any).m = g;\nconst a: any = new A(); const b: any = new B(); const c: any = new C();\na.m(); b.m(); c.m();\n", "m", true);
    assert!(
        r.iter().all(|x| x == "T0Mono"),
        "a.m() and c.m() call A::m only, b.m() g only: {r:?}"
    );
}

// ---- AUDFIX2 KEEP cases: neighbouring shapes that stay proven.

/// U2 keep: a store of another key on the instance does not relink it: `a.m()` is A::m alone.
#[test]
fn audit1_u2_keep_other_key_store() {
    let r = v(
        "class A { m() { return 1; } }\nconst a: any = new A();\na.zz = { m: g };\na.m();\n",
        "m",
        true,
    );
    assert!(has(&r, "T0Mono"), "a.m() calls A::m alone: {r:?}");
}
/// U2 keep: a `__proto__` store on one class's instance leaves an unrelated class's calls proven.
#[test]
fn audit1_u2_keep_unrelated_class() {
    let r = v("class A { m() { return 1; } }\nclass Q { q() { return 2; } }\nconst a: any = new A();\na.__proto__ = { m: g };\nnew Q().q();\n", "q", true);
    assert!(has(&r, "T0Mono"), "new Q().q() calls Q::q alone: {r:?}");
}
/// U2d: the destructuring target form runs the same setter.
#[test]
fn audit1_u2d_proto_store_destructuring() {
    let r = v("class A { m() { return 1; } }\nconst a: any = new A();\n({ x: a.__proto__ } = { x: { m: g } });\na.m();\n", "m", true);
    assert!(!has(&r, "T0Mono"), "a.m() calls g in JS: {r:?}");
}

/// U3 keep: `Reflect.get` with receiver = target keeps the getter's `this.m()` proven (P::m alone).
#[test]
fn audit1_u3_keep_receiver_is_target() {
    let r = v("class P { m() { return 1; } get gg(): any { return this.m(); } }\nconst p = new P();\nReflect.get(p, \"gg\", p);\n", "m", true);
    assert!(has(&r, "T0Mono"), "this.m() calls P::m alone: {r:?}");
}
/// U4 keep: `Reflect.set` with receiver = target keeps the setter's `this.m()` proven.
#[test]
fn audit1_u4_keep_receiver_is_target() {
    let r = v("class P { m() { return 1; } set ss(x: any) { this.m(); } }\nconst p = new P();\nReflect.set(p, \"ss\", 1, p);\n", "m", true);
    assert!(has(&r, "T0Mono"), "this.m() calls P::m alone: {r:?}");
}
/// U5 KEEP: the descriptor of a user object own data property gives its value only:
/// `X.m()` has the verdict of `o.a` (or `undefined`) read directly.
#[test]
fn audit1_u5_keep_user_data_descriptor() {
    let r = v("const o = { a: { m() { return 1; } } };\nconst X: any = (Object.getOwnPropertyDescriptor(o, \"a\") as any).value;\nX.m();\n", "m", true);
    let w = v(
        "const o = { a: { m() { return 1; } } };\nconst X: any = g() ? o.a : undefined;\nX.m();\n",
        "m",
        true,
    );
    assert!(
        r == w && !has(&r, "Throws"),
        "X is o.a or undefined: {r:?} {w:?}"
    );
}
/// U8 gaps: `Object.assign(B, src)` does Set(B, key, v) for each own enumerable key of `src`
/// (20.1.2.1); a key that may be `"__proto__"` runs the setter (B.2.2.1.2) and relinks class B:
/// `new D()` then constructs through the new parent, so `b.x` is not the field of A alone.
fn u8_gap(maker: &str) -> (Vec<String>, Vec<String>) {
    let src = format!("class A {{ x: any = {{ zz: 1 }}; }}\nclass C {{}}\nclass B extends A {{}}\nclass D extends B {{}}\nconst go = () => {{ const b: any = new D(); b.x.zz; }};\nconst k: any = String(g());\n{maker}\ngo();\n");
    (v(&src, "x", false), v(&src, "zz", false))
}
/// (a) `JSON.parse` makes own keys by CreateDataProperty (25.5.1), any string of the text.
#[test]
fn audit1_u8_json() {
    let (x, z) = u8_gap("Object.assign(B, JSON.parse(k));");
    assert!(
        !has(&x, "T0Mono") && !has(&z, "T0Mono"),
        "B may be relinked to a JSON object: {x:?} {z:?}"
    );
}
/// (b) `Object.fromEntries` (20.1.2.7) CreateDataPropertyOrThrow under the entry key.
#[test]
fn audit1_u8_from_entries() {
    let (x, z) = u8_gap("Object.assign(B, Object.fromEntries([[k, C]]));");
    assert!(
        !has(&x, "T0Mono") && !has(&z, "T0Mono"),
        "B may be relinked to C: {x:?} {z:?}"
    );
}
/// (c) `Object.defineProperty(s, k, { value })` (20.1.2.4) defines an own key `k`.
#[test]
fn audit1_u8_define_property() {
    let (x, z) = u8_gap("const s: any = {};\nObject.defineProperty(s, k, { value: C, enumerable: true });\nObject.assign(B, s);");
    assert!(
        !has(&x, "T0Mono") && !has(&z, "T0Mono"),
        "B may be relinked to C: {x:?} {z:?}"
    );
}
