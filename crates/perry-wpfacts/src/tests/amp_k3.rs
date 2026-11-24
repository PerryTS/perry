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

// K3 (B), receiver family (owner 11-23): a method or accessor that code
// outside the program's view gets hold of runs only with `this` = an
// object of its class family (its home holder, the objects whose chain
// reaches it, the objects it was copied onto under its key, Proxies of
// them) or nullish; never ⊤.

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

/// T2: an escaped object literal's method and getter run on the literal.
#[test]
fn k3b_t2_escaped_literal_method_this_is_its_family() {
    let src = format!(
        "\"use strict\";\n{DECL}const o = {{ w: 2, m() {{ return /*A*/ this.w; }} }};\nconst q = {{ w: 3, get g() {{ return /*B*/ this.w; }} }};\no.m(); q.g;\nsink(o.m); sink(Object.getOwnPropertyDescriptor(q, \"g\")); sink(o); sink(q);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "w");
    assert!(!top && r.len() == 1, "an escaped literal method's this is ⊤: {v} {r:?}");
    let (v, top, r) = recv_after(&src, "/*B*/", "w");
    assert!(!top && r.iter().all(|n| n.contains("lit") || n.contains("descriptor")), "an escaped literal getter's this is ⊤: {v} {r:?}");
    // The escaped descriptor record holds the getter under `get`: whoever
    // holds the record can call `d.get()` on it (a copy target).
    assert!(r.iter().any(|n| n.contains("descriptor")), "the escaped descriptor record is missing from the getter's this: {r:?}");
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

/// T4 control: program code calls the method with a `this` outside its
/// family: the method keeps ⊤ `this` as before.
#[test]
fn k3b_t4_this_outside_the_family_keeps_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\np.m.call({{ z: 1 }});\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(top && !v.starts_with("T0"), "a method called on a non-member lost its ⊤ this: {v} {r:?}");
}

/// T4b (CWN ruling, family premise 11-23 "objects it was copied onto"):
/// an object holding the method under another key is a copy target: its
/// program call runs on it, the family stays on.
#[test]
fn k3b_t4b_holder_under_another_key_joins_the_family() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\nconst h = {{ h: p.m }};\nh.h();\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 2, "a holder under another key switched the family off: {v} {r:?}");
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
/// FromPropertyDescriptor) holds the method under `value`: whoever holds
/// the record can call `d.value()` on it, so it joins the family; the
/// program's own `d.value()` runs on it too.
#[test]
fn k3b_t10_descriptor_record_joins_the_family() {
    for tail in ["sink(o.m); sink(d); sink(o);", "sink(d); sink(o);"] {
        let src = format!(
            "\"use strict\";\n{DECL}const o = {{ w: 2, m() {{ return /*A*/ this.w; }} }};\no.m();\nconst d: any = Object.getOwnPropertyDescriptor(o, \"m\");\nd.value();\n{tail}\n"
        );
        let (v, top, r) = recv_after(&src, "/*A*/", "w");
        assert!(!top && r.len() == 2, "{tail}: the descriptor record is no family member: {v} {r:?}");
        assert!(r.iter().any(|n| n.contains("descriptor")), "{tail}: the record is missing from this: {r:?}");
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

/// T12 (CWN ruling): a method stored under a computed key into an object
/// that escapes: `this` = its family and that object, no ⊤.
#[test]
fn k3b_t12_computed_key_holder_joins_the_family() {
    let src = format!(
        "{DECL}declare const k: string;\nclass P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nconst h: any = {{}};\nh[k] = p.m;\nsink(h);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(!top && r.len() == 2, "a computed-key holder switched the family off: {v} {r:?}");
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
