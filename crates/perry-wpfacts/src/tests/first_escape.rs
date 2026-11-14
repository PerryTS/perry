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

/// ARRSRC (r7, head 7aac20dd17): the hold token BuiltinFn(Any) in
/// InterleavedBuffer::clone / BufferAttribute::copy `new this.array.constructor(..)`
/// is the `.constructor` of a Blob (`builtins/loads.rs` undescribed member,
/// anyfall `(Opaque, Some("Blob.prototype")) | "constructor"`). The Blob reaches
/// `.array` exactly: GLTFLoader.js:3188 `new TypedArray(bufferView, ..)` with
/// TypedArray = WEBGL_COMPONENT_TYPES[json key] = Object (an unknown JSON string
/// key may be "constructor") returns bufferView (ECMA-262 20.1.1.1), and
/// bufferView = `buffer.slice(..)` may be a Blob. Blob.prototype.constructor is
/// %Blob% (WebIDL interface object), so `new` of it is a known construction.
fn blob_ctor_escapes(ctor: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        function id(x: any): any {{ return x; }}
        const b: any = new Blob(["ab"]);
        const s: any = id(b).slice(0, 1);
        const C: any = {ctor};
        const v = new V();
        const w: any = new C(["c"], v);
        console.log(w.size, v.probe());
        "#
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

/// (PROTOCTOR: the Blob reaches `.slice` through `id`, as r7's bufferView
/// reaches it through a value; a direct `b.slice` lowers to the native
/// module call `blob.slice`, whose result is ⊤: a separate gap.)
/// RED on 7aac20dd17: the constructor of a Blob is %Blob%; `new Blob(parts,
/// options)` reads only `type`/`endings` of V (absent on V and its chain), so V
/// stays proven.
#[test]
fn red_arrsrc_new_of_a_blob_constructor_is_known() {
    assert!(!blob_ctor_escapes("s.constructor"));
}

/// Control (can fail): an unknown host constructor really is unknown.
#[test]
fn control_arrsrc_new_of_a_host_constructor_escapes() {
    assert!(blob_ctor_escapes("(globalThis as any).hostGet()"));
}

/// PROTOCTOR: `new C(args)` where C is a built-in constructor value read
/// from a realm chain; V passed as an argument stays proven when `new` is
/// the known construction, escapes when it is unknown.
fn protoctor_escapes(setup: &str, ctor: &str, args: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        {setup}
        const C: any = {ctor};
        const v = new V();
        const w: any = new C({args});
        console.log(typeof w, v.probe());
        "#
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

/// The global `Blob` value and a Blob's `constructor` are one token.
#[test]
fn protoctor_global_blob_value_constructs() {
    assert!(!protoctor_escapes("", "Blob", "[\"c\"], v"));
}

/// `(new ArrayBuffer(8)).constructor` is %ArrayBuffer%: `new` of it is an
/// ArrayBuffer (ToIndex of V reads V's chain only), no hold.
#[test]
fn protoctor_array_buffer_constructor_is_array_buffer() {
    assert!(!protoctor_escapes("const ab: any = new ArrayBuffer(8);", "ab.constructor", "v"));
}

/// `[].constructor` is %Array%.
#[test]
fn protoctor_array_constructor_is_array() {
    assert!(!protoctor_escapes("const a: any = [];", "a.constructor", "v"));
}

/// The runtime's array-iterator chain holds no `constructor`
/// (%ArrayIteratorPrototype%, %IteratorPrototype%): the read finds
/// `Object.prototype.constructor`, %Object%; `new Object(v)` is v.
#[test]
fn protoctor_chain_without_constructor_is_object() {
    assert!(!protoctor_escapes(
        "const it: any = [][Symbol.iterator]();",
        "it.constructor",
        "v"
    ));
}

/// Control (can fail): a program store of an unknown value into
/// `Blob.prototype.constructor` keeps the unknown constructor.
#[test]
fn control_protoctor_written_blob_constructor_escapes() {
    assert!(protoctor_escapes(
        "(Blob.prototype as any).constructor = (globalThis as any).hostGet(); const b: any = new Blob([\"ab\"]);",
        "b.constructor",
        "[\"c\"], v"
    ));
}

/// PROTOINH: a member a runtime-declared prototype does not own is read on
/// the realm chain (`Blob.prototype` -> `Object.prototype`, realm-v1.json):
/// `Object.prototype.isPrototypeOf` read as a value off a Blob and called
/// with V reads V's [[Prototype]] chain only (20.1.3.3), so V stays proven
/// and the realm is not held. Before: the read was an undescribed member
/// (BuiltinFn(Any)), its call an unknown function: V escaped.
fn protoinh_escapes(setup: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        function id(x: any): any {{ return x; }}
        {setup}
        const b: any = new Blob(["ab"]);
        const s: any = id(b).slice(0, 1);
        const v = new V();
        const o: any = {{ m: s.isPrototypeOf, h: s.hasOwnProperty }};
        console.log(o.m(v), o.h("x"), v.probe());
        "#
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

#[test]
fn red_protoinh_inherited_member_of_a_blob_is_object_prototypes() {
    assert!(!protoinh_escapes(""));
}

/// Control (can fail): a program store of an unknown value onto
/// `Blob.prototype` under the key (an add the runtime admits: Blob.prototype
/// is not add-locked, perry-abi lock.rs) is what the read finds.
#[test]
fn control_protoinh_written_blob_member_escapes() {
    assert!(protoinh_escapes(
        "(Blob.prototype as any).isPrototypeOf = (globalThis as any).hostGet();"
    ));
}

/// Control (can fail): unknown code holds the realm first (V's own escape
/// aside, a host call given a Blob may add `isPrototypeOf` to
/// Blob.prototype): the inherited read is then not exact.
#[test]
fn control_protoinh_held_realm_member_escapes() {
    assert!(protoinh_escapes(
        "(globalThis as any).hostGet()(Blob.prototype);"
    ));
}

/// ABSCALL (r7 hold after PROTOINH, GLTFLoader.js:4447
/// `node.position.fromArray(nodeDef.translation)`): the receiver set holds a
/// program object with `fromArray` and an ArrayBuffer. No object of the
/// ArrayBuffer's chain (the instance, `ArrayBuffer.prototype`,
/// `Object.prototype`) holds `fromArray`, so [[Get]] is `undefined` and the
/// call throws a TypeError before any argument is used (10.1.8.1, 13.3.6.2
/// EvaluateCall, 7.3.14 Call): V given as the argument never escapes, and the
/// program method's `this` holds only the program instance. GREEN on
/// ed2c278107 (collections.rs bobj_method, the ArrayBuffer absent arm): pins.
fn abscall_proven(src_recv: &str, wrap: bool) -> (bool, bool) {
    let call = "const r: any = id(json.c ? new P() : RECV);\n            const v = new V();\n            console.log(r.fromArray(v), v.probe());".replace("RECV", src_recv);
    let body = if wrap {
        format!("fetch(json.uri).then((r: any) => r.arrayBuffer()).then((hb: any) => {{\n            {call}\n        }});")
    } else {
        call
    };
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        class P {{ px = 0; fromArray(a: any) {{ return this.px; }} }}
        function id(x: any): any {{ return x; }}
        const json: any = JSON.parse(process.argv[2] ?? "{{}}");
        {body}
        "#
    );
    let pf = facts(&s);
    (
        !matches!(site(&pf, "zz").verdict, Verdict::No(_)),
        !matches!(site(&pf, "px").verdict, Verdict::No(_)),
    )
}

#[test]
fn abscall_absent_member_of_array_buffer_is_a_type_error() {
    assert_eq!(abscall_proven("new ArrayBuffer(8)", false), (true, true));
}

/// The r7 kind of buffer (FileLoader: `fetch(..).then(r => r.arrayBuffer())`,
/// WHATWG Fetch: an ArrayBuffer).
#[test]
fn abscall_absent_member_of_fetched_buffer_is_a_type_error() {
    assert_eq!(abscall_proven("hb", true), (true, true));
}

/// R7HOLD4 RED, the r7 source (`Blob.prototype@call2455`: the Blob a
/// `Response.prototype.blob()` fulfills with, three ImageBitmapLoader's
/// `res.blob()`). In perry a Blob is a native handle: it owns no enumerable
/// property but the ones the program stores on it (perry-runtime
/// object/field_get_set/enumeration.rs:203-222), and `Object.assign` and
/// spread copy nothing from it (object/assign.rs:779-792, alloc.rs:1197).
/// So the copy gives `obj` no unknown key and `fromArray` stays `V`'s.
#[test]
fn red_r7hold4_assign_from_fetched_blob_stores_nothing() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } fromArray(a: any) { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        const obj: any = json.c ? new V() : new ArrayBuffer(8);
        async function main() {
            const b: any = await new Response("x").blob();
            Object.assign(obj, b);
            obj.fromArray([1, 2, 3]);
        }
        main();
        console.log(new V().probe());
    "#;
    let v = site(&facts(s), "zz").verdict.clone();
    assert!(!matches!(v, Verdict::No(_)), "V escapes: {v:?}");
}

/// Control (i) of R7HOLD4: a host object (`process`) owns enumerable
/// properties the model does not list: the copy gives `obj` unknown keys,
/// so `obj.fromArray` may be any function and `V` escapes.
#[test]
fn control_r7hold4_assign_from_host_object_escapes() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } fromArray(a: any) { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        const obj: any = json.c ? new V() : new ArrayBuffer(8);
        Object.assign(obj, process);
        obj.fromArray([1, 2, 3]);
        console.log(new V().probe());
    "#;
    assert!(matches!(site(&facts(s), "zz").verdict, Verdict::No(_)));
}

/// Control (ii) of R7HOLD4: what the program stores on the Blob is its own
/// enumerable property (enumeration.rs:203-222): a copy of it carries the
/// stored value, here an unknown function, so `V` escapes.
#[test]
fn control_r7hold4_assign_from_blob_carries_its_expando() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } fromArray(a: any) { return this.zz; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        const obj: any = json.c ? new V() : new ArrayBuffer(8);
        async function main() {
            const b: any = await new Response("x").blob();
            b.fromArray = (globalThis as any)[json.k];
            Object.assign(obj, b);
            obj.fromArray([1, 2, 3]);
        }
        main();
        console.log(new V().probe());
    "#;
    assert!(matches!(site(&facts(s), "zz").verdict, Verdict::No(_)));
}

/// The realm is held: a call of the realm's `Math.sin` read through a value
/// is no IntrinsicCall (`new_verdicts::intrinsic_call_control_realm_held`).
fn r7hold5_realm_held(pf: &ProgramFacts) -> bool {
    pf.sites.iter().any(|s| {
        s.key == "()"
            && matches!(&s.verdict, Verdict::No(r) if r.starts_with("intrinsic callee: unknown code holds the realm") || r == "callee set holds ⊤")
    })
}

/// R7HOLD5 RED, the r7 shape: `p.getX(1)` (r7.ts, `p` read off an `any`)
/// whose callee set holds the global `Blob` (call c1126, ns Other), and
/// GLTFLoader's `new Blob([bufferView], {type: mimeType})`. A call of the
/// `Blob` value runs `global_this_blob_thunk` (perry-runtime
/// object/global_this/fetch_globals.rs:305-313; `this` unread), whose effects
/// the realm declares (global_this_tables.rs:206): parts read by index, each
/// part bytes or ToString, `options.type` ToString (perry-stdlib
/// fetch_blob.rs:74-147, 195-202). No unknown code runs: the realm is not held.
#[test]
fn red_r7hold5_call_of_blob_value_runs_its_declared_effects() {
    let s = r#"
        class V { zz = 1; probe() { return this.zz; } getX(i: number) { return this.zz + i; } }
        const json: any = JSON.parse(process.argv[2] ?? "{}");
        const p: any = json.c ? new V() : { getX: Blob };
        const bufferView = new Uint8Array(4);
        const blob = new Blob([bufferView], { type: "image/png" });
        p.getX(1);
        const f = Math.sin;
        const y = f(1);
        console.log(new V().probe(), blob, y);
    "#;
    let pf = facts(s);
    assert!(!r7hold5_realm_held(&pf), "realm held: {}", pf.dump_text());
}

/// Control (i): a part that is a program object with `toString`: the
/// coercion runs it with the part as `this` (fetch_blob.rs:144); no hold.
#[test]
fn control_r7hold5_blob_part_to_string_runs() {
    let s = r#"
        const part = { tag: "x", toString() { return this.tag; } };
        const g: any = Blob;
        g([part]);
        const f = Math.sin;
        const y = f(1);
        console.log(y);
    "#;
    let pf = facts(s);
    assert!(!r7hold5_realm_held(&pf), "realm held: {}", pf.dump_text());
    assert!(!matches!(site(&pf, "tag").verdict, Verdict::No(_)), "{}", pf.dump_text());
}

/// Control (ii): a parts value with a program `Symbol.iterator`. The runtime
/// never calls it (fetch_blob.rs:74-108: an array is read by index, any other
/// value is one part, coerced whole); the declaration's `Iterate`
/// (global_this_tables.rs:206) is a superset that may run it. So the exact
/// assertions: no hold, and the iterator's read is not Dead (it may run). Its
/// verdict is not asserted: the literal's computed key holds its layout.
#[test]
fn control_r7hold5_blob_parts_iterator_runs() {
    let s = r#"
        const parts = { n: 1, [Symbol.iterator]() { const k = this.n; return [k][Symbol.iterator](); } };
        const g: any = Blob;
        g(parts);
        const f = Math.sin;
        const y = f(1);
        console.log(y);
    "#;
    let pf = facts(s);
    assert!(!r7hold5_realm_held(&pf), "realm held: {}", pf.dump_text());
    assert!(!matches!(site(&pf, "n").verdict, Verdict::Dead), "{}", pf.dump_text());
}

/// Control (iii): a part unknown code made: its ToString is unknown code,
/// which holds the realm.
#[test]
fn control_r7hold5_blob_part_from_unknown_code_holds() {
    let s = r#"
        declare function ext(): any;
        const g: any = Blob;
        g([ext()]);
        const f = Math.sin;
        const y = f(1);
        console.log(y);
    "#;
    assert!(r7hold5_realm_held(&facts(s)));
}

/// Control (iv): a global constructor the realm does not declare effects
/// for (`File`, global_this_tables.rs:207), called as a value: unknown code;
/// the hold stays.
#[test]
fn control_r7hold5_undeclared_ctor_value_call_holds() {
    let s = r#"
        const g: any = File;
        g([1], "n");
        const f = Math.sin;
        const y = f(1);
        console.log(y);
    "#;
    assert!(r7hold5_realm_held(&facts(s)));
}

/// R7HOLD7: `V` escapes when the read under `name` (after a store of an
/// unknown function under `key` into the JSON Array `a` and
/// `Object.assign(o, a)`) is called with a `V`.
fn r7hold7_escapes(key: &str, read: &str, assign: bool) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const j: any = JSON.parse(process.argv[2] ?? "{{}}");
        const i: number = Number(process.argv[3]);
        const a: any = j.arr;
        const o: any = j.obj;
        const f: any = (globalThis as any)[j.k];
        a[{key}] = f;
        {}
        o{read}(new V());
        console.log(new V().probe());
        "#,
        if assign { "Object.assign(o, a);" } else { "" }
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

/// R7HOLD7 RED, the r7 shape (GLTFLoader.js:2366 `Object.assign(object.userData,
/// gltfDef.extras)` with `extras` a JSON Array whose elements
/// InterleavedBufferAttribute.setX wrote, `this.data.array[index * stride +
/// offset] = x`). The elements of an Array are its own properties under
/// array-index keys (ECMA-262 10.4.2); `Object.assign` copies them under those
/// keys (20.1.2.1, 7.3.25 CopyDataProperties): ToString of an index is digits,
/// never `position`. So `o.position` never reads the stored function.
#[test]
fn red_r7hold7_assign_from_array_copies_elements_under_index_keys() {
    assert!(!r7hold7_escapes("i * j.stride + 1", ".position", true));
}

/// Control (i): a JSON String key may name `position`: the copy carries it.
#[test]
fn control_r7hold7_string_key_store_is_copied_under_its_name() {
    assert!(r7hold7_escapes("j.k", ".position", true));
}

/// Control (ii): `'' + j.offset` is a String that may be `position`.
#[test]
fn control_r7hold7_concat_key_store_is_copied_under_its_name() {
    assert!(r7hold7_escapes("'' + j.offset", ".position", true));
}

/// Control (iii): a Number-only key into the object itself writes its
/// numeric range, never `position` (existing).
#[test]
fn control_r7hold7_number_key_without_copy_no_escape() {
    assert!(!r7hold7_escapes("i * 2 + 1", ".position", false));
}

/// Control (iv): `1 + j.s` may be the String `"1x"` (j.s = "x"): a member
/// really named so is read after the copy.
#[test]
fn control_r7hold7_numeric_prefix_name_is_copied() {
    assert!(r7hold7_escapes("1 + j.s", "[\"1x\"]", true));
}

/// R7HOLD8 Part A: `i * j.stride + j.offset` with `j.offset` a JSON value: if
/// it is a String the sum is a String (ECMA-262 13.15.3
/// ApplyStringOrNumericBinaryOperator: either primitive a String ->
/// concatenation), e.g. `"1x"` (i*stride = 1, offset = "x"). The key of the
/// element store may therefore be a non-index name: `a["1x"]` may read `f`.
fn r7hold8_sum_key_escapes(key: &str, read: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const j: any = JSON.parse(process.argv[2] ?? "{{}}");
        const i: number = Number(process.argv[3]);
        const a: any = j.arr;
        const f: any = (globalThis as any)[j.k];
        const k = {key};
        a[k] = f;
        a{read}(new V());
        console.log(new V().probe());
        "#
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

#[test]
fn r7hold8_number_plus_maybe_string_is_maybe_a_string() {
    assert!(r7hold8_sum_key_escapes("i * j.stride + j.offset", "[\"1x\"]"));
}

/// Control: a Number-only sum (`i * j.stride + 1`) is an index key: the
/// element store never names `"1x"`, so the read never sees `f`.
#[test]
fn control_r7hold8_number_only_sum_never_names_a_non_index() {
    assert!(!r7hold8_sum_key_escapes("i * j.stride + 1", "[\"1x\"]"));
}

/// R7HOLD9 rule 1 pin: `new Blob(x)` converts `x` as a `sequence<BlobPart>`
/// (WebIDL: GetMethod(x, @@iterator), then the iterator it returns is
/// stepped, ECMA-262 7.4.2/7.4.4); `next` is called on that iterator, never
/// on `x` itself. `x` has no `@@iterator` (nor has Object.prototype): the
/// conversion throws before any `next`, so `x` never reaches `u`.
fn r7hold9_blob_part_escapes(part: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const u: any = (globalThis as any)[process.argv[2] ?? "x"];
        const w = new V();
        const x: any = {part};
        try {{ new Blob(x); }} catch (e) {{}}
        console.log(w.probe());
        "#
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

#[test]
fn red_r7hold9_blob_part_next_is_not_called_on_the_iterable() {
    assert!(!r7hold9_blob_part_escapes("{ next: u, w }"));
}

/// R7HOLD10: `x`'s own `@@iterator` returns `{ next: u, w }`. The
/// runtime never runs the iteration protocol on Blob parts: `js_blob_new`
/// (perry-stdlib fetch_blob.rs `append_blob_parts`) reads an Array's
/// elements by index (`js_array_get`) and takes any other value as ONE part
/// (`append_one_blob_part`: a Blob's or buffer's bytes, else ToString). So
/// neither `@@iterator` nor `u` is called and `w` stays local.
#[test]
fn red_r7hold10_blob_part_iterator_is_never_called() {
    assert!(!r7hold9_blob_part_escapes(
        "{ [Symbol.iterator]() { return { next: u, w }; } }"
    ));
}

/// Control: an element of an Array part is converted by ToString
/// (`append_one_blob_part`, declared `coerce(elems(arg(0)), ToPrimitive)`):
/// its `toString` runs and hands `w` to unknown code.
#[test]
fn control_r7hold10_blob_array_element_to_string_runs() {
    assert!(r7hold9_blob_part_escapes(
        r#"[{ toString() { u(w); return "p"; } }]"#
    ));
}

/// Control: a non-array part is converted by ToString as one part: its
/// own `toString` runs and hands `w` to unknown code.
#[test]
fn control_r7hold10_blob_single_part_to_string_runs() {
    assert!(r7hold9_blob_part_escapes(
        r#"{ toString() { u(w); return "p"; } }"#
    ));
}

/// R7HOLD9 rule 2: `ArrayBuffer.prototype.resize` called with a `this` that
/// is no ArrayBuffer throws a TypeError before it reads anything (ECMA-262
/// 25.1.6.6 step 2 RequireInternalSlot(O, [[ArrayBufferMaxByteLength]]);
/// the runtime: typed_array.rs `array_buffer_method`, brand check first):
/// `o` never reaches unknown code and the call has no value.
fn r7hold9_resize_escapes(recv: &str) -> bool {
    let s = format!(
        r#"
        class V {{ zz = 1; probe() {{ return this.zz; }} }}
        const o: any = {{ m: ArrayBuffer.prototype.resize, w: new V() }};
        const buf: any = new ArrayBuffer(8, {{ maxByteLength: 16 }});
        buf.m = o.m;
        buf.w = o.w;
        const c = process.argv.length > 3;
        let r: any = undefined;
        try {{ r = ({recv}).m(8); }} catch (e) {{}}
        console.log(o.w.probe(), r);
        "#
    );
    matches!(site(&facts(&s), "zz").verdict, Verdict::No(_))
}

#[test]
fn red_r7hold9_arraybuffer_method_on_a_non_buffer_throws() {
    assert!(!r7hold9_resize_escapes("o"));
}

/// Control: `this` possibly the real buffer (`c ? o : buf`, `buf` holding
/// the same `m` and `w`): the buffer case keeps its model, so the verdict
/// is the buffer-only one, whatever that model gives.
#[test]
fn control_r7hold9_arraybuffer_method_on_a_buffer_keeps_its_model() {
    assert!(!r7hold9_resize_escapes("buf"));
}

/// The receiver either `o` or the buffer: each runs on its own (`o` throws,
/// the buffer keeps its model): the verdict is the buffer-only one.
#[test]
fn red_r7hold9_arraybuffer_method_on_a_buffer_or_a_non_buffer() {
    assert!(!r7hold9_resize_escapes("c ? o : buf"));
}

