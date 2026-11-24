//! K3 (owner premises Phase B and receiver family, 2026-11-23): a callback
//! handed to a builtin or the host runs only at its contract's invoke
//! points. (A): an escape a builtin contract states runs no invoke of its
//! own; the arm binds the contract's invokes as program calls. A value
//! later handed to code that may write it gets that code's calls (⊤
//! arguments) once.
use super::*;

const DECL: &str = "declare function sink(x: any): void;\n";

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    crate::compute(&[(path, &hir)], &cfg)
}

/// The verdict of the first `.key` site (read or call) after `marker`.
fn site_after(src: &str, marker: &str, key: &str, call: bool) -> String {
    let pf = one(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("no {key} site after {marker:?}"));
    let v = format!("{:?}", s.verdict);
    eprintln!("AMP-K3 {src:?}: {key}: {v}");
    v
}

const CB: &str = "function cb(e: any) { return /*A*/ e.n; }\n";

/// (A): the stream calls its write callback with no argument
/// (`schedule_write_callback`); nothing else calls it, so `e` is no ⊤.
#[test]
fn k3a_stream_callback_parameter_is_no_top() {
    let src = format!("{CB}process.stdout.write(\"x\", cb as any);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(!v.starts_with("No("), "the contract callback's parameter is ⊤: {v}");
}

/// (A): a chunk that is no string is converted by ToString; its
/// `toString` runs with `this` = the chunk, not ⊤.
#[test]
fn k3a_stream_chunk_to_string_runs_on_the_chunk() {
    let src = "const o = { s: \"a\", toString() { return /*A*/ this.s; } };\nprocess.stdout.write(o as any);\n";
    let v = site_after(src, "/*A*/", "s", false);
    assert!(!v.starts_with("No("), "the chunk's toString ran on ⊤: {v}");
}

/// T5: an escape no contract states (fetch's `escapes(field(arg(1), body))`)
/// keeps ⊤ parameters for what it hands.
#[test]
fn k3_t5_undeclared_escape_keeps_top_params() {
    let src = "const b = { m(e: any) { return /*A*/ e.n; } };\nfetch(\"u\", { body: b } as any);\n";
    let v = site_after(src, "/*A*/", "n", false);
    assert!(v.starts_with("No("), "an undeclared escape's callback lost its ⊤ parameter: {v}");
}

/// T6: handed first by contract (stream write), then to unknown code: the
/// re-escape runs the writer part the contract escape skipped (⊤ params).
#[test]
fn k3_t6_contract_then_writer_runs_the_writer_part() {
    let src = format!("{DECL}{CB}process.stdout.write(\"x\", cb as any);\nsink(cb);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(v.starts_with("No("), "contract-then-writer kept the contract view: {v}");
}

/// T6 control: the same writer escape alone gives ⊤ params.
#[test]
fn k3_t6_control_writer_alone() {
    let src = format!("{DECL}{CB}sink(cb);\n");
    let v = site_after(&src, "/*A*/", "n", false);
    assert!(v.starts_with("No("), "a writer escape kept the parameter: {v}");
}

// The owner's premise (11-23), verbatim: "A method or getter/setter that
// code outside the program's view gets hold of runs only with `this` = an
// object of its own class family. The family includes classes it was
// copied onto, and Proxies that forward to such an object." + "a violation
// must be caught by a check at the runtime's callback/accessor invoke
// point, never on proven sites". Owner 11-24: the bare class prototype is
// not a family object. CWN K3BUILD10: a plain holder (no prototype) is no
// member; a program call through it is a modelled call (`this` = it by its
// binding); only handed calls (a builtin or the host calls) are checked.

/// The first `.key` read after `marker`: (verdict, the unguarded receiver
/// set holds ⊤, the unguarded receivers). The family tests read the solved
/// set: a class method's site is E0 behind its entry guard whether `this`
/// holds ⊤ or not (design §11.4), so the verdict alone does not tell them
/// apart.
fn recv_after(src: &str, marker: &str, key: &str) -> (String, bool, Vec<String>) {
    let pf = one(src);
    let at = src.find(marker).expect("marker") as u32;
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call && s.byte_offset >= at)
        .min_by_key(|s| s.byte_offset)
        .unwrap_or_else(|| panic!("no {key} site after {marker:?}"));
    let v = format!("{:?}", s.verdict);
    let top = s.receiver_values.iter().any(|r| matches!(r.kind, crate::model::TokKind::Top));
    let vals: Vec<String> = s.receiver_values.iter().map(|r| r.display.clone()).collect();
    eprintln!("AMP-K3B {marker} {key}: {v} top={top} prims={:#x} recv={vals:?}", s.receiver_prims);
    (v, top, vals)
}

/// T1: an escaped class method reads its own class's field exactly.
#[test]
fn k3b_t1_escaped_class_method_this_is_its_family() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top, "an escaped class method's this is ⊤: {v} {r:?}");
}

/// T2: an escaped object literal's method runs on the literal; its getter
/// keeps ⊤ (CWN ruling 3).
#[test]
fn k3b_t2_escaped_literal_method_this_is_its_family() {
    let src = format!(
        "\"use strict\";\n{DECL}const o = {{ w: 2, m() {{ return /*A*/ this.w; }} }};\nconst q = {{ w: 3, get g() {{ return /*B*/ this.w; }} }};\no.m(); q.g;\nsink(o.m); sink(Object.getOwnPropertyDescriptor(q, \"g\")); sink(o); sink(q);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "w");
    assert!(!top && r.len() == 1, "an escaped literal method's this is ⊤: {v} {r:?}");
    // CWN ruling 3: an accessor gets no family `this` (the runtime cannot
    // check a getter a builtin runs until its getter path is split from
    // program reads): the escaped getter keeps ⊤.
    let (v, top, r) = recv_after(&src, "/*B*/", "w");
    assert!(top, "an escaped literal getter got a family this: {v} {r:?}");
}

/// T3: a method copied onto another class's prototype under its key runs
/// on that class's instances too: `this` holds both classes.
#[test]
fn k3b_t3_copy_target_joins_the_family() {
    let base = "class P { x = 1; m() { return /*A*/ this.x; } }\nclass Q { y = 0; x = 3; }\n";
    let only = format!("{DECL}{base}new P().m(); sink(new Q());\nsink(P.prototype.m);\n");
    let copied =
        format!("{DECL}{base}(Q.prototype as any).m = P.prototype.m;\nnew P().m(); sink(new Q());\nsink(P.prototype.m);\n");
    let (a, at, ra) = recv_after(&only, "/*A*/", "x");
    let (b, bt, rb) = recv_after(&copied, "/*A*/", "x");
    assert!(!at, "an escaped class method's this is ⊤: {a} {ra:?}");
    assert!(!bt, "a copied method's this is ⊤: {b} {rb:?}");
    let q = |r: &[String]| r.iter().any(|n| n.contains(":Q"));
    assert!(!q(&ra), "Q instances in this without the copy: {ra:?}");
    assert!(q(&rb), "the copy target's instances are missing from this: {rb:?}");
}

/// T3b: copied onto a class prototype under another key ("classes it was
/// copied onto"): that class's instances join; a plain object holding it
/// joins only as a prototype (`Object.create(h)`), never itself.
#[test]
fn k3b_t3b_copy_onto_a_prototype_under_another_key_joins() {
    let base = "class P { x = 1; m() { return /*A*/ this.x; } }\nclass Q { y = 0; x = 3; }\n";
    let src = format!("{DECL}{base}(Q.prototype as any).k = P.prototype.m;\nnew P().m(); sink(new Q());\nsink(P.prototype.m);\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.iter().any(|n| n.contains(":Q")), "the instances of a class it was copied onto are missing: {v} {r:?}");
    let src = format!("{DECL}{base}const h: any = {{ x: 4 }};\nh.k = P.prototype.m;\nconst c = Object.create(h);\nnew P().m(); sink(c); sink(h);\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 2, "Object.create(h) of a holder under another key: {v} {r:?}");
    assert!(!r.iter().any(|n| n.starts_with("lit")), "the plain holder itself is in this: {r:?}");
}

/// T7 (CWN ruling 2): `new f()` of a plain function takes its chain from
/// `f.prototype`: a method copied onto it runs on those instances too.
#[test]
fn k3b_t7_new_function_instance_chain_joins_the_family() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nfunction F(this: any) {{ this.x = 5; }}\n(F.prototype as any).m = P.prototype.m;\nnew P().m(); const i = new (F as any)(); i.m();\nsink(P.prototype.m);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top, "a copied method's this is ⊤: {v} {r:?}");
    assert!(r.iter().any(|n| n.contains("F")), "the new F() instances are missing from this: {r:?}");
}

/// T7b control: an escaped `new F()` instance hands unknown code `F`
/// (`i.constructor`), which may replace the writable `F.prototype`
/// (10.2.6 MakeConstructor writablePrototype true): the copy's holder may
/// be an object from unmodelled code, so `this` is ⊤.
#[test]
fn k3b_t7b_copy_onto_writable_prototype_of_escaped_fn_keeps_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nfunction F(this: any) {{ this.x = 5; }}\n(F.prototype as any).m = P.prototype.m;\nnew P().m(); sink(new (F as any)());\nsink(P.prototype.m);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(top && !v.starts_with("T0"), "a copy onto an unknown holder lost its ⊤ this: {v} {r:?}");
}

/// T2c (CWN ruling 1): a literal property whose value is a function
/// expression is a method of the literal, as `{ f() {} }` is.
#[test]
fn k3b_t2c_literal_function_expression_is_a_method() {
    let src = format!(
        "\"use strict\";\n{DECL}const o = {{ w: 2, m: function () {{ return /*A*/ this.w; }} }};\no.m();\nsink(o.m); sink(o);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "w");
    assert!(!top && r.len() == 1, "an escaped literal function property's this is ⊤: {v} {r:?}");
}

/// T8: unknown code calls an escaped method only on what it holds: a
/// family member that never escaped is not its receiver there.
#[test]
fn k3b_t8_unheld_member_is_no_receiver() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && !r.iter().any(|n| n.starts_with("proto:")), "an unheld member (P.prototype) in the escaped run's this: {v} {r:?}");
}

/// T4: program code calls the method with a `this` outside its family
/// (`.call` is a modelled call, never checked): `this` is that receiver by
/// its binding beside the family, no ⊤.
#[test]
fn k3b_t4_program_call_outside_the_family_is_its_binding() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\np.m.call({{ z: 1 }});\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 2, "a program .call outside the family switched it off: {v} {r:?}");
}

/// T15: a handed call (a builtin calls it: `forEach`'s thisArg) with a
/// `this` outside the family: the runtime check would reject it, so the
/// family is off and `this` ⊤; with a member as thisArg it stays on.
#[test]
fn k3b_t15_handed_call_outside_the_family_keeps_top() {
    // `[1].forEach(f, t)` lowers to the array-like method (HIR binds the
    // callback's `this` there); a Set's `forEach` is the builtin call.
    let base = "class P { x = 1; m() { return /*A*/ this.x; } }\nconst p = new P();\np.m();\nsink(P.prototype.m);\nconst a = new Set<number>([1]);\n";
    for recv in ["[1]", "a"] {
        let src = format!("{DECL}{base}{recv}.forEach(p.m, {{ z: 1 }});\n");
        let (v, top, r) = recv_after(&src, "/*A*/", "x");
        assert!(top, "{recv}: a handed call on a non-member lost its ⊤ this: {v} {r:?}");
    }
    // The array-like model also calls the receiver's members (its
    // `Protocol::Any` implicit call, the callback stored as an element):
    // that handed call runs `m` on the array, so an array receiver puts the
    // family off; the Set's builtin call keeps it.
    let src = format!("{DECL}{base}a.forEach(p.m, p);\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top, "a handed call on a member switched the family off: {v} {r:?}");
}

/// T4b (CWN K3BUILD10): a plain object holding the method under another
/// key is no member: escaped alone it is no receiver; the program's own
/// `h.h()` runs on it by its binding; no ⊤ either way.
#[test]
fn k3b_t4b_plain_holder_under_another_key_is_no_member() {
    let base = "class P { x = 1; m() { return /*A*/ this.x; } }\nconst p = new P();\np.m();\nsink(P.prototype.m);\nconst h = { h: p.m };\n";
    let src = format!("{DECL}{base}sink(h);\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 1, "an escaped plain holder is in this: {v} {r:?}");
    let src = format!("{DECL}{base}h.h();\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 2, "the program call through the holder: {v} {r:?}");
}

// K3BUILD7: a class constructor's `prototype` is non-writable and
// non-configurable (ECMA-262 15.7.14 ClassDefinitionEvaluation:
// MakeConstructor(F, false, proto)): no write by unknown code that holds
// the class replaces it.

/// T9: unknown code that may write any key of class `X` leaves
/// `X.prototype` X's prototype object.
#[test]
fn k3b_t9_escaped_class_prototype_stays_its_own() {
    let src = format!("{DECL}class X {{ m() {{ return 1; }} }}\nsink(X);\nconst p = X.prototype;\n/*A*/ p.z;\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "z");
    assert!(!top && r.iter().all(|n| n.starts_with("proto:")), "an escaped class's prototype read holds more than its prototype: {v} {r:?}");
}

/// T9b: a subclass of an escaped class takes its parent prototype from
/// `X.prototype`: its prototype's chain holds X's prototype, no ⊤.
#[test]
fn k3b_t9b_subclass_of_escaped_class_has_no_top_chain() {
    let src = format!("{DECL}class X {{ m() {{ return 1; }} }}\nsink(X);\nconst ns = {{ X }};\nclass Y extends ns.X {{ r() {{ return 2; }} }}\nconst q = Object.getPrototypeOf(Y.prototype);\n/*A*/ q.z;\n");
    let (v, top, r) = recv_after(&src, "/*A*/", "z");
    assert!(!top && r.iter().all(|n| n.starts_with("proto:")), "an escaped class's subclass chain holds more than its prototype: {v} {r:?}");
}

/// T10: an escaped property descriptor record (6.2.6.4
/// FromPropertyDescriptor) holds the method under `value`: a plain holder,
/// no member; the program's own `d.value()` runs on it by its binding.
#[test]
fn k3b_t10_descriptor_record_is_no_member() {
    // `d.value()` resolves no call (d.value holds ⊤ beside m, K3BUILD7):
    // the record is in `this` by neither path.
    for call in ["", "d.value();\n"] {
        let src = format!(
            "\"use strict\";\n{DECL}const o = {{ w: 2, m() {{ return /*A*/ this.w; }} }};\no.m();\nconst d: any = Object.getOwnPropertyDescriptor(o, \"m\");\n{call}sink(o.m); sink(d); sink(o);\n"
        );
        let (v, top, r) = recv_after(&src, "/*A*/", "w");
        assert!(!top && r.len() == 1, "{call:?}: {v} {r:?}");
        assert!(!r.iter().any(|n| n.contains("descriptor")), "{call:?}: the record in this: {r:?}");
    }
}

/// T11 (owner 11-24): the bare class prototype is no object of its class
/// family for `this`: an escaped `P.prototype` is no receiver there.
#[test]
fn k3b_t11_bare_prototype_is_no_receiver() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype); sink(p);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && !r.is_empty() && !r.iter().any(|n| n.starts_with("proto:")), "the bare prototype is in the escaped run's this: {v} {r:?}");
}

/// T12 (CWN K3BUILD10): a method stored under a computed key into a plain
/// object that escapes: no member, `this` = its family, no ⊤.
#[test]
fn k3b_t12_computed_key_plain_holder_is_no_member() {
    let src = format!(
        "{DECL}declare const k: string;\nclass P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nconst h: any = {{}};\nh[k] = p.m;\nsink(h);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 1, "a computed-key plain holder is in this: {v} {r:?}");
}

/// T12 control: a method stored into an object from unmodelled code (⊤,
/// no token): that object may be `this`, so `this` is ⊤.
#[test]
fn k3b_t12b_stored_into_top_keeps_top() {
    let src = format!(
        "{DECL}declare const k: string;\ndeclare function get(): any;\nclass P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nget()[k] = p.m;\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(top && !v.starts_with("T0"), "a method stored into ⊤ lost its ⊤ this: {v} {r:?}");
}

/// T13 (CWN Q2): a value stored into an object from unmodelled code (⊤)
/// is watched for later arrivals: a method that escaped elsewhere and
/// reaches it after ⊤ did is dropped beside that ⊤ (`escaped_beside_top`)
/// and still leaves its family. The receiver-rooted path (DECISIONS 149b,
/// `rooted_store`) is watched the same way (`rfam_top_holder` runs first).
#[test]
fn k3b_t13_later_arrival_at_a_top_held_value_keeps_top() {
    let src = format!(
        "{DECL}declare function get(): any;\nclass P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nfunction put(v: any) {{ get().k = v; }}\nput(get());\nconst p = new P();\np.m();\nsink(p.m);\nconst w = {{ f: p }};\nfunction later(o: any) {{ return o.f; }}\nput(later(w).m);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(top && !v.starts_with("T0"), "a method reaching a value stored into ⊤ lost its ⊤ this: {v} {r:?}");
}

/// T16 (CWN ruling 3): an escaped class getter and setter keep ⊤ `this`;
/// the class's method gets its family.
#[test]
fn k3b_t16_class_accessor_has_no_family() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} get g() {{ return /*B*/ this.x; }} set s(v: number) {{ /*C*/ this.x; }} }}\nconst p = new P();\np.m(); p.g; p.s = 2;\nsink(P.prototype.m); sink(Object.getOwnPropertyDescriptor(P.prototype, \"g\")); sink(Object.getOwnPropertyDescriptor(P.prototype, \"s\")); sink(p);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top, "an escaped class method's this is ⊤: {v} {r:?}");
    for at in ["/*B*/", "/*C*/"] {
        let (v, top, r) = recv_after(&src, at, "x");
        assert!(top, "{at}: an escaped class accessor got a family this: {v} {r:?}");
    }
}

/// T11b (CWN ruling 2): a subclass prototype is no receiver of its
/// family either; its instances are.
#[test]
fn k3b_t11b_subclass_prototype_is_no_receiver() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nclass Q extends P {{ y = 2; }}\nconst q = new Q();\nq.m();\nsink(P.prototype.m); sink(Q.prototype); sink(q);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && !r.is_empty(), "{v} {r:?}");
    assert!(!r.iter().any(|n| n.starts_with("proto:")), "a class prototype is in the escaped run's this: {r:?}");
}

/// T17 (C3): a live family's function is listed for the runtime check with
/// its home key; a function whose family a handed call put off is not.
#[test]
fn k3c_t17_live_family_is_listed_with_its_home_key() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return this.x; }} }}\nclass Q {{ y = 1; n() {{ return this.y; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\nconst q = new Q();\nq.n();\nsink(Q.prototype.n);\nnew Set<number>([1]).forEach(q.n, {{ z: 1 }});\n"
    );
    let pf = one(&src);
    let live = pf.recv_family_live.expect("the solve completed");
    eprintln!("AMP-K3 T17 {live:?}");
    assert!(live.values().any(|k| k == &["m".to_string()]), "P::m is not listed live: {live:?}");
    assert!(!live.values().any(|k| k.iter().any(|k| k == "n")), "Q::n (family off) is listed: {live:?}");
}
