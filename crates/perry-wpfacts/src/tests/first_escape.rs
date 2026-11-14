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


/// R7HOLD (r7 on the merge 3a1663b291, hold at pops 54,123): GLTFLoader.js:3230
/// `bufferAttribute.array.slice()` (loadAccessor, sparse branch) is a call whose
/// receiver node `.array` holds TOP (plus ArrayBuffer, Object, the Object.prototype
/// methods, Object(empty)/BoxedString/TypedArray of the `new TypedArray(..)` site).
/// The array is `new TypedArray(bufferView, byteOffset, n)` with TypedArray =
/// WEBGL_COMPONENT_TYPES[json key] (GLTFLoader.js:3188/3207), stored by the
/// BufferAttribute constructor. Every value of `TypedArray` is a known function:
/// a typed-array constructor, or for an inherited key Object (20.1.1.1: `new
/// Object(ab)` is ab itself, ToObject of a primitive, or a fresh object) or a
/// non-constructor (`new` throws, 13.3.5.1.1 step 7). So the `slice` callee is a
/// closed set of builtins, the argument is converted by ToIntegerOrInfinity
/// (23.2.3.27 / 25.1.6.6), and V never escapes.
fn r7_v_escapes(array_src: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        const TYPES: any = {{ 5120: Int8Array, 5121: Uint8Array, 5126: Float32Array }};
        class BA {{ array: any; constructor(array: any) {{ this.array = array; }} }}
        const bufferView: any = json.b ? new ArrayBuffer(16) : null;
        {array_src}
        const ba = new BA(array);
        const r = ba.array.slice(new V() as any);
        console.log(r.length, new V().probe());
        "#
    );
    let v = site(&facts(&s), "zz").verdict.clone();
    matches!(v, Verdict::No(_))
}

#[test]
fn r7hold_json_keyed_typed_array_new_slice() {
    assert!(!r7_v_escapes(
        "const TypedArray = TYPES[json.componentType];\n        const array = bufferView === null ? new TypedArray(json.count * 3) : new TypedArray(bufferView, json.o || 0, json.count * 3);"
    ));
}

/// Control: one known constructor, no keyed read: nothing escapes.
#[test]
fn control_r7hold_fixed_typed_array_no_escape() {
    assert!(!r7_v_escapes("const array = new Float32Array(bufferView ?? 4);"));
}

/// Control (can fail): a host value as the array: the slice callee is unknown,
/// V is given to unknown code and escapes.
#[test]
fn control_r7hold_host_array_escapes() {
    assert!(r7_v_escapes("const array = (globalThis as any).hostGet();"));
}

/// R7HOLD control (real, (a)): the bufferView is a truly unknown host value (the FileLoader/fetch
/// result, TOP in the model). With TypedArray = Object (the inherited
/// `constructor` of the keyed read) `new Object(⊤)` is ⊤ itself under the
/// NEWOBJ rule (b825aead1f), so `.array` holds ⊤ and the `slice` callee is
/// unknown: for componentType "constructor" the program really calls the host
/// value's slice with V, so V escapes. Compare the control below: the same host buffer through a fixed
/// typed-array constructor.
#[test]
fn control_r7hold_keyed_new_of_host_buffer_escapes() {
    assert!(r7_v_escapes(
        "const hb: any = (globalThis as any).hostBuf();\n        const TypedArray = TYPES[json.componentType];\n        const array = new TypedArray(hb, json.o || 0, json.count * 3);"
    ));
}

/// Control: the same host buffer through one typed-array constructor: the
/// result is a typed array (23.2.5.1), its slice is known, V does not escape.
#[test]
fn control_r7hold_fixed_ctor_of_host_buffer_no_escape() {
    assert!(!r7_v_escapes(
        "const hb: any = (globalThis as any).hostBuf();\n        const array = new Float32Array(hb, json.o || 0, json.count * 3);"
    ));
}

/// R7HOLD pin, GREEN on 3a1663b291 (the r7 shape, FileLoader three.core.js 45108/45209): the buffer
/// is `fetch(url)` -> Response -> `response.arrayBuffer()`, an ArrayBuffer by
/// WHATWG Fetch (Body mixin, consume body: ArrayBuffer). Every `TypedArray`
/// value then yields a typed array, the ArrayBuffer itself (`new Object(ab)`,
/// 20.1.1.1 step 3) or a throw: the slice callee is closed, V never escapes.
#[test]
fn r7hold_keyed_new_of_fetched_buffer_slice() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        const TYPES: any = { 5120: Int8Array, 5121: Uint8Array, 5126: Float32Array };
        class BA { array: any; constructor(array: any) { this.array = array; } }
        fetch(json.uri).then((r: any) => r.arrayBuffer()).then((hb: any) => {
            const TypedArray = TYPES[json.componentType];
            const ba = new BA(new TypedArray(hb, json.o || 0, json.count * 3));
            const r = ba.array.slice(new V() as any);
            console.log(r.length, new V().probe());
        });
    "#;
    let v = site(&facts(s), "zz").verdict.clone();
    assert!(!matches!(v, Verdict::No(_)), "V escapes: {v:?}");
}

/// R7HOLD2 RED (r7, GLTFLoader.js:577 GLTFRegistry + :2928 getDependency):
/// `objects[key]` in a registry whose `key` parameter only ever holds
/// strings `"bufferView:" + n` (ECMA-262 13.15.3 ApplyStringOrNumericBinaryOperator:
/// every value contains ":"; no `Object.prototype` property name does,
/// 20.1.3) can read no inherited member: `hb` is the ArrayBuffer or
/// undefined, `hb.constructor` is `ArrayBuffer` (25.1.4) and the `new` is
/// `new ArrayBuffer(4)`; V is passed to `ArrayBuffer.prototype.slice`
/// (25.1.6.6, ToIntegerOrInfinity: the ordinary valueOf/toString) and never
/// escapes. The solver reads the inherited `constructor` (Object), whose
/// `constructor` is the Function constructor; `new` of it is ⊤.
#[test]
fn r7hold2_registry_colon_key_reads_no_inherited_member() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        function Reg() {
            let objects: any = {};
            return {
                get: function (key: any) { return objects[key]; },
                add: function (key: any, o: any) { objects[key] = o; },
            };
        }
        const cache = Reg();
        cache.add("bufferView:" + json.i, new ArrayBuffer(16));
        if (!cache.get("scene")) cache.add("scene", Promise.all([]));
        let ck = "ClonedMaterial:" + json.u + ":";
        if (json.f) ck += "flat-shading:";
        if (!cache.get(ck)) cache.add(ck, { m: 1 });
        const hb = cache.get("bufferView:" + json.j);
        const C = hb.constructor;
        const r = new C(4).slice(new V() as any);
        console.log(r.byteLength, new V().probe());
    "#;
    let v = site(&facts(s), "zz").verdict.clone();
    assert!(!matches!(v, Verdict::No(_)), "V escapes: {v:?}");
}

/// Control: the key is any string of the input, so `hb` may be the host
/// value stored under "bufferView:"+i (unknown, ⊤): `new hb.constructor(4)`
/// is unknown and V escapes. Real. (Its ⊤ no longer comes from `new` of the
/// Function constructor: without the interpreter that throws, 20.2.1.1.)
#[test]
fn control_r7hold2_registry_any_key_escapes() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        function Reg() {
            let objects: any = {};
            return {
                get: function (key: any) { return objects[key]; },
                add: function (key: any, o: any) { objects[key] = o; },
            };
        }
        const cache = Reg();
        cache.add("bufferView:" + json.i, (globalThis as any).hostBuf());
        const hb = cache.get(json.k);
        const C = hb.constructor;
        const r = new C(4).slice(new V() as any);
        console.log(r.byteLength, new V().probe());
    "#;
    let v = site(&facts(s), "zz").verdict.clone();
    assert!(matches!(v, Verdict::No(_)), "V must escape: {v:?}");
}

// FNCTOR: the `constructor` of a function value is %Function% (ECMA-262
// 20.2.3.2); %AsyncFunction%, %GeneratorFunction%, %AsyncGeneratorFunction%
// come only from async / generator function values. Without the interpreter
// (FactsConfig::default(): no dynamic_code, `interp_linked` false) `new
// Function(x)` converts x with ToString, then throws a TypeError
// (perry-runtime js_function_ctor_from_strings_impl -> refuse_dynamic_function):
// it gives no value.
fn fnctor_v_escapes(src: &str, interp: bool) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        {src}
        console.log(new V().probe());
        "#
    );
    let mut cfg = FactsConfig::default();
    if interp {
        cfg.dynamic_code = Some("new Function(...) @ x.ts (names globals)".into());
    }
    let v = site(&facts_with(&s, cfg), "zz").verdict.clone();
    matches!(v, Verdict::No(_))
}

/// RED on ab6e892d68: `Object.constructor` is %Function%; `new` of it throws.
#[test]
fn fnctor_new_of_object_constructor_gives_nothing() {
    let src = "const C: any = (Object as any).constructor; const r = new C(4); r.slice(new V() as any);";
    assert!(!fnctor_v_escapes(src, false));
}

/// RED on ab6e892d68: the three.js shape (three.core.js:20585) with a
/// receiver that is a typed array or `Object`.
#[test]
fn fnctor_new_of_receiver_constructor_typed_array_or_object() {
    let src = "const arr = [1, 2]; const o: any = json.a ? new Float32Array(4) : Object; \
               const r = new o.constructor(arr); r.slice(new V() as any);";
    assert!(!fnctor_v_escapes(src, false));
}

/// Control (i): CreateDynamicFunction runs ToString on each argument
/// (20.2.1.1.1 step 10; the runtime's function_ctor_arg_strings): the
/// program `toString` runs, and what it stores escapes.
#[test]
fn control_fnctor_argument_to_string_runs() {
    let src = "class W { toString() { (globalThis as any).hostTake(new V()); return \"a\"; } } \
               const C: any = (Object as any).constructor; try { new C(new W()); } catch (e) {}";
    assert!(fnctor_v_escapes(src, false));
}

/// Control (ii): with the interpreter linked the constructed function runs
/// unknown code: the result stays unknown and V escapes.
#[test]
fn control_fnctor_interpreter_result_is_unknown() {
    let src = "const C: any = (Object as any).constructor; const r = new C(json.s); r.slice(new V() as any);";
    assert!(fnctor_v_escapes(src, true));
}

/// Control (iii): `(async function(){}).constructor` is %AsyncFunction%:
/// its `new` makes a runtime object (construct.rs generic path, prototype
/// %AsyncFunction.prototype%) the model does not describe: unknown, V escapes.
#[test]
fn control_fnctor_async_function_constructor_is_its_own() {
    let src = "async function af() {} const C: any = (af as any).constructor; \
               const r: any = new C(4); r.v = new V(); (globalThis as any).hostTake(r);";
    assert!(fnctor_v_escapes(src, false));
}

/// Control (iv): a real typed array's `constructor` is its typed-array
/// constructor; `new` of it is a typed array, no unknown value.
#[test]
fn control_fnctor_typed_array_constructor_is_typed_array() {
    let src = "const ta: any = new Float32Array(4); const r = new ta.constructor(4); r.slice(new V() as any);";
    assert!(!fnctor_v_escapes(src, false));
}
