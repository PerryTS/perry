//! K3 (owner premise Phase B, 2026-11-23): a callback handed to a builtin
//! or the host runs only at its contract's invoke points. (A): an escape a
//! builtin contract states runs no invoke of its own; the arm binds the
//! contract's invokes as program calls. A value later handed to code that
//! may write it gets that code's calls (⊤ arguments) once.
//! (B), the receiver-family narrowing of an escaped method's `this`, is
//! removed (owner 11-26): no runtime check backs it. The (B) tests state
//! the sound behaviour: unknown code calls a method it holds on any
//! receiver, so its `this` holds ⊤.
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

// K3 (B) removed (owner 11-26): the receiver-family premise (an escaped
// method runs only on its class family) has no runtime check behind it,
// so the solver does not use it. Unknown code that holds a method or an
// accessor may call it on any receiver it holds or makes: `this` holds ⊤.

/// The first `.key` read after `marker`: (verdict, the unguarded receiver
/// set holds ⊤, the unguarded receivers). A class method's site is E0
/// behind its entry guard whether `this` holds ⊤ or not (design §11.4), so
/// the verdict alone does not tell them apart.
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

/// The escaped run's `this` at the first `.key` read after `marker` holds
/// ⊤ and the read is not proven over the known receivers alone.
fn assert_top_this(src: &str, marker: &str, key: &str, what: &str) {
    let (v, top, r) = recv_after(src, marker, key);
    assert!(top && !v.starts_with("T0"), "{what}: this lost ⊤: {v} {r:?}");
}

/// Red test (K3OFF): a method handed to unknown code that also holds an
/// object of its own: unknown code may call `m` on that object
/// (`m.call(o)`), so `this` holds ⊤. The family narrowing proved the read
/// over `P` alone; the same program with `new Function(...)` as the
/// unknown code printed the foreign object's first slot (`0`) instead of
/// `foreign`.
#[test]
fn k3off_foreign_receiver_from_unknown_code_keeps_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\nsink({{ y: 0, x: \"foreign\" }});\n"
    );
    assert_top_this(&src, "/*A*/", "x", "a foreign receiver unknown code holds");
}

/// T1 (was: this is its family): an escaped class method's `this` is ⊤.
#[test]
fn k3b_t1_escaped_class_method_this_is_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\n"
    );
    assert_top_this(&src, "/*A*/", "x", "an escaped class method");
}

/// T2 (was: this is its family): an escaped object literal's method and
/// getter (handed out through its descriptor record) run on any receiver.
#[test]
fn k3b_t2_escaped_literal_method_this_is_top() {
    let src = format!(
        "\"use strict\";\n{DECL}const o = {{ w: 2, m() {{ return /*A*/ this.w; }} }};\nconst q = {{ w: 3, get g() {{ return /*B*/ this.w; }} }};\no.m(); q.g;\nsink(o.m); sink(Object.getOwnPropertyDescriptor(q, \"g\")); sink(o); sink(q);\n"
    );
    assert_top_this(&src, "/*A*/", "w", "an escaped literal method");
    assert_top_this(&src, "/*B*/", "w", "an escaped literal getter");
}

/// T3 (was: the copy target joins the family): an escaped method, copied
/// or not, runs on any receiver.
#[test]
fn k3b_t3_copied_escaped_method_this_is_top() {
    let base = "class P { x = 1; m() { return /*A*/ this.x; } }\nclass Q { y = 0; x = 3; }\n";
    let only = format!("{DECL}{base}new P().m(); sink(new Q());\nsink(P.prototype.m);\n");
    let copied =
        format!("{DECL}{base}(Q.prototype as any).m = P.prototype.m;\nnew P().m(); sink(new Q());\nsink(P.prototype.m);\n");
    assert_top_this(&only, "/*A*/", "x", "an escaped class method");
    assert_top_this(&copied, "/*A*/", "x", "a copied escaped method");
}

/// T7 (was: a `new f()` instance chain joins the family): the copy onto
/// `F.prototype` of an escaped method runs on any receiver.
#[test]
fn k3b_t7_new_function_instance_copy_this_is_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nfunction F(this: any) {{ this.x = 5; }}\n(F.prototype as any).m = P.prototype.m;\nnew P().m(); const i = new (F as any)(); i.m();\nsink(P.prototype.m);\n"
    );
    let (v, top, r) = recv_after(&src, "/*A*/", "x");
    assert!(top && !v.starts_with("T0"), "a copied escaped method lost ⊤ this: {v} {r:?}");
    assert!(r.iter().any(|n| n.contains("F")), "the program's own call on new F() is missing from this: {r:?}");
}

/// T7b control: an escaped `new F()` instance hands unknown code `F`
/// (`i.constructor`), which may replace the writable `F.prototype`
/// (10.2.6 MakeConstructor writablePrototype true): `this` is ⊤.
#[test]
fn k3b_t7b_copy_onto_writable_prototype_of_escaped_fn_keeps_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nfunction F(this: any) {{ this.x = 5; }}\n(F.prototype as any).m = P.prototype.m;\nnew P().m(); sink(new (F as any)());\nsink(P.prototype.m);\n"
    );
    assert_top_this(&src, "/*A*/", "x", "a copy onto an unknown holder");
}

/// T2c (was: a literal function expression is a method of its family): an
/// escaped literal function property runs on any receiver.
#[test]
fn k3b_t2c_literal_function_expression_this_is_top() {
    let src = format!(
        "\"use strict\";\n{DECL}const o = {{ w: 2, m: function () {{ return /*A*/ this.w; }} }};\no.m();\nsink(o.m); sink(o);\n"
    );
    assert_top_this(&src, "/*A*/", "w", "an escaped literal function property");
}

/// T4 control: program code calls the method with a `this` of its own:
/// `this` is ⊤ (escaped) and holds that object.
#[test]
fn k3b_t4_this_outside_the_family_keeps_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\np.m.call({{ z: 1 }});\n"
    );
    assert_top_this(&src, "/*A*/", "x", "a method called on another object");
}

/// T4b (was: a holder under another key joins the family): the escaped
/// method held under another key runs on any receiver.
#[test]
fn k3b_t4b_holder_under_another_key_this_is_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype.m);\nconst h = {{ h: p.m }};\nh.h();\n"
    );
    assert_top_this(&src, "/*A*/", "x", "an escaped method held under another key");
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

/// T10 (was: the descriptor record joins the family): an escaped
/// property descriptor record (6.2.6.4 FromPropertyDescriptor) hands the
/// method out under `value`: unknown code may call it on any receiver.
#[test]
fn k3b_t10_descriptor_record_escape_this_is_top() {
    for tail in ["sink(o.m); sink(d); sink(o);", "sink(d); sink(o);"] {
        let src = format!(
            "\"use strict\";\n{DECL}const o = {{ w: 2, m() {{ return /*A*/ this.w; }} }};\no.m();\nconst d: any = Object.getOwnPropertyDescriptor(o, \"m\");\nd.value();\n{tail}\n"
        );
        assert_top_this(&src, "/*A*/", "w", tail);
    }
}

/// T11 (was owner 11-24: the bare class prototype is no receiver; that
/// relied on the runtime family check, which does not exist): an escaped
/// `P.prototype` hands out `m`, and unknown code may call it on anything.
#[test]
fn k3b_t11_escaped_prototype_method_this_is_top() {
    let src = format!(
        "{DECL}class P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nsink(P.prototype); sink(p);\n"
    );
    assert_top_this(&src, "/*A*/", "x", "a method of an escaped prototype");
}

/// T12 (was: a computed-key holder joins the family): a method stored
/// under a computed key into an object that escapes runs on any receiver.
#[test]
fn k3b_t12_computed_key_holder_escape_this_is_top() {
    let src = format!(
        "{DECL}declare const k: string;\nclass P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nconst h: any = {{}};\nh[k] = p.m;\nsink(h);\n"
    );
    assert_top_this(&src, "/*A*/", "x", "a method under a computed key of an escaped holder");
}

/// T12 control: a method stored into an object from unmodelled code (⊤,
/// no token): `this` is ⊤.
#[test]
fn k3b_t12b_stored_into_top_keeps_top() {
    let src = format!(
        "{DECL}declare const k: string;\ndeclare function get(): any;\nclass P {{ x = 1; m() {{ return /*A*/ this.x; }} }}\nconst p = new P();\np.m();\nget()[k] = p.m;\n"
    );
    assert_top_this(&src, "/*A*/", "x", "a method stored into ⊤");
}
