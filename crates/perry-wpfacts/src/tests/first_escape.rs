//! TRACE110 (skin m60s / t110-prov, head 08a3806c85): the solve's FIRST
//! escape. three.core.js:13254 (Object3D.toJSON, BatchedMesh branch)
//! `this._geometryInfo.map(info => ({ ...info, boundingBox: .., boundingSphere: .. }))`
//! = {lit#110}. The typed-array `map` model (builtins/typed_array_methods.rs:209)
//! runs ToPrimitive on the callback result (23.2.3.22 step 6.c -> 7.1.4), so
//! @@toPrimitive/valueOf/toString are looked up on the literal. Its any-key field
//! holds TOP, seeded by the spread (solve/fields.rs spread_into, "spread of an
//! unmodeled object") of `info` = `Object()@call3915`: GLTFLoader.js:3188
//! `new TypedArray(bufferView, ..)` with TypedArray = WEBGL_COMPONENT_TYPES[json key],
//! which may be Object.prototype.constructor = Object. `new Object(v)` is modelled
//! (builtins/constructors.rs:282) as an opaque fresh "Object()" wrapper; ECMA-262
//! 20.1.1.1 gives a definite value: v itself (an object), a fresh ordinary object
//! with no own property (undefined/null), or ToObject(v) (a primitive).
use super::*;

fn v_escapes(src: &str, extra: &str, host: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        {src}
        const lit: any = {{ ...o, v: new V(){extra} }};
        new Int8Array(2).map(() => lit);
        {host}
        console.log(new V().probe());
        "#
    );
    let v = site(&facts(&s), "zz").verdict.clone();
    matches!(v, Verdict::No(_))
}

const NEW_OBJECT: &str = "const o: any = new Object(json.a ? new ArrayBuffer(8) : null);";

/// RED on 08a3806c85: `new Object(null | ArrayBuffer)` has knowable own
/// properties (none / the ArrayBuffer's), so the spread copies no TOP, the
/// ToPrimitive of the literal finds only Object.prototype methods, and V
/// never escapes.
#[test]
fn red_trace110_spread_of_new_object_no_escape() {
    assert!(!v_escapes(NEW_OBJECT, "", ""));
}

/// Control: a real host value spread into the literal: its keys and values
/// are unknown, so the protocol call is of an unknown function: V escapes.
#[test]
fn control_trace110_host_value_spread_escapes() {
    assert!(v_escapes("const o: any = (globalThis as any).hostGet();", "", ""));
}

/// Control: a program-defined protocol method is called with the literal as
/// `this` and hands it to unknown code: V escapes.
#[test]
fn control_trace110_program_valueof_escapes() {
    assert!(v_escapes(
        NEW_OBJECT,
        ", valueOf() { (globalThis as any).hostTake(this); return 0; }",
        ""
    ));
}

/// NEW-OBJECT-VALUE: `new Object(a)` of an object is `a` itself (20.1.1.1
/// step 3, ToObject 7.1.18): a store through the result is a store on `a`,
/// so the host read of `a.v` hands V out. RED while the result is a fresh
/// opaque object.
#[test]
fn newobj_object_arg_is_itself() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } }
        const a: any = { v: 0 };
        const o: any = new Object(a);
        o.v = new V();
        (globalThis as any).hostTake(a.v);
        console.log(new V().probe());
    "#;
    assert!(matches!(site(&facts(s), "zz").verdict, Verdict::No(_)));
}

/// The call form `Object(a)` (NewTarget undefined): the same identity.
#[test]
fn newobj_call_form_object_arg_is_itself() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } }
        const a: any = { v: 0 };
        const o: any = Object(a);
        o.v = new V();
        (globalThis as any).hostTake(a.v);
        console.log(new V().probe());
    "#;
    assert!(matches!(site(&facts(s), "zz").verdict, Verdict::No(_)));
}

/// `new Object(null)` / `Object(undefined)`: an ordinary object with no own
/// property, so the spread adds no key and V never escapes.
#[test]
fn newobj_null_spreads_no_key() {
    assert!(!v_escapes("const o: any = new Object(null);", "", ""));
    assert!(!v_escapes("const o: any = Object(undefined);", "", ""));
    assert!(!v_escapes("const o: any = new Object();", "", ""));
}

/// A Number or Boolean wrapper owns no property (21.1.1.1, 20.3.1.1).
#[test]
fn newobj_number_boolean_wrapper_spreads_no_key() {
    assert!(!v_escapes("const o: any = new Object(json.n ? 1 : true);", "", ""));
}

/// Control: a later program store on the empty object is tracked like on
/// `{}`: a V stored there is spread into the literal, and a host take of
/// the literal's field hands it out.
#[test]
fn control_newobj_store_on_empty_object_is_seen() {
    assert!(v_escapes(
        "const o: any = new Object(null); o.w = new V();",
        "",
        "(globalThis as any).hostTake(lit.w);"
    ));
}

/// Control: ⊤ in the argument stays ⊤ in the result (an external value):
/// the spread copies unknown keys, V escapes.
#[test]
fn control_newobj_top_arg_stays_top() {
    assert!(v_escapes("const o: any = new Object((globalThis as any).hostGet());", "", ""));
}

/// OCVALUE: `Object.create(proto)` called as a VALUE (through a variable or
/// a member read result) is the same operation as the named call (ECMA-262
/// 20.1.2.2: OrdinaryObjectCreate(proto)): the result's chain is P, and P
/// is given to no one. RED on 0cbf487a3d: the value call hands P to unknown
/// code (`unknown_method_at`), so V escapes.
fn ocvalue_zz_proven(call: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        const P: any = {{ v: new V() }};
        {call}
        console.log(o.v.probe());
        "#
    );
    !matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

/// Green before the rule too: generation resolves a const binding of the
/// builtin to the named call.
#[test]
fn ocvalue_object_create_through_variable() {
    assert!(ocvalue_zz_proven("const c: any = Object.create; const o: any = c(P);"));
}

#[test]
fn red_ocvalue_object_create_through_member_read() {
    assert!(ocvalue_zz_proven(
        "const k: any = { f: Object.create }; const o: any = k.f(P);"
    ));
}

/// `Function.prototype.call` (20.2.3.3) calls the same function with the
/// remaining arguments.
#[test]
fn red_ocvalue_object_create_through_call() {
    assert!(ocvalue_zz_proven(
        "const k: any = { f: Object.create }; const o: any = k.f.call(undefined, P);"
    ));
}

/// Control: the callee may also be a program function that hands its
/// argument to the host: that function is still called, V escapes.
#[test]
fn control_ocvalue_program_function_is_still_called() {
    assert!(!ocvalue_zz_proven(
        "const c: any = json.k ? Object.create : (x: any) => { (globalThis as any).hostTake(x); return x; }; const o: any = c(P);"
    ));
}

