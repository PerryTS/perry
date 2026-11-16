//! TRACE4162: three.js `convertArray(array, type)` (three.core.js:41717) runs
//! `new type(array)` only where `typeof type.BYTES_PER_ELEMENT === "number"`.
//! `Array` has no `BYTES_PER_ELEMENT` (ECMA-262 23.1.2: from, isArray, of,
//! prototype, @@species; Function.prototype 20.2.3 has none either), so
//! `typeof Array.BYTES_PER_ELEMENT` is "undefined" (13.5.3) and the branch never
//! constructs an Array. The model constructs one anyway (`Array@call395`, whose
//! one element is the argument: 23.1.1.1 with one non-Number argument), and the
//! skin root 8815 descends from that element (a Float32Array read as a value
//! of `values[i]`, then a computed read of it, an undescribed builtin function,
//! the `then` the promise resolution of `Promise.all([...])` at
//! GLTFLoader.js:4162 calls). Here the element is an Array whose expando holds
//! `svc`; only an element that is not there reads it.
use super::*;

const CONVERT: &str = r#"if (!array || array.constructor === type) return array;
            if (typeof type.BYTES_PER_ELEMENT === "number") return new type(array);
            return Array.prototype.slice.call(array);"#;

fn verdict(host: bool, decls: &str, ty: &str, arg: &str, after: &str, key: &str) -> Verdict {
    verdict_conv(host, decls, CONVERT, ty, arg, after, key)
}

fn verdict_conv(
    host: bool,
    decls: &str,
    body: &str,
    ty: &str,
    arg: &str,
    after: &str,
    key: &str,
) -> Verdict {
    let src = format!(
        r#"{imp}
        declare const ext: any;
        declare const Host: any;
        class Svc {{ size = 3; zz = 1; }}
        const svc: any = new Svc();
        {decls}
        function convert(array: any, type: any) {{
            {body}
        }}
        const T: any = {ty};
        const r: any = convert({arg}, T);
        {after}
        function g(a: any) {{ return a.size; }}
        console.log(g(svc));
        "#,
        imp = if host { "import \"unseen-host\";" } else { "" }
    );
    let pf = facts(&src);
    site(&pf, key).verdict.clone()
}

fn escapes(decls: &str, ty: &str, arg: &str, after: &str) -> bool {
    matches!(verdict(true, decls, ty, arg, after, "size"), Verdict::No(_))
}

/// `r[0].s` read by `h` beside an `Other`: one receiver class unless `r[0]`
/// may be `nums` (whose `s` is `svc`).
fn elem_mono(ty: &str) -> bool {
    elem_mono_with("", ty)
}

fn elem_mono_with(extra: &str, ty: &str) -> bool {
    let decls = &format!("{extra} class Other {{ zz = 2; }} const nums: any = [1, 2]; nums.s = svc;
        function h(o: any) {{ return o.zz; }}");
    let after = "const x: any = r[0]; console.log(h(x.s), h(new Other()));";
    matches!(verdict(false, decls, ty, "nums", after, "zz"), Verdict::T0Mono)
}

/// The skin hop: `type` holds Float32Array and Array; only Float32Array has a
/// Number `BYTES_PER_ELEMENT`, so `new type(array)` makes no Array and `r[0]`
/// is never `nums` itself, only its Numbers (`new Float32Array(nums)`, `nums`
/// returned as it is, or `slice` of it): `r[0].s` is never `svc`.
#[test]
fn typeof_member_guard_skips_builtin_array_ctor() {
    assert!(elem_mono("Math.random() < 2 ? Float32Array : Array"));
}

/// The same rule on program classes: a class without a Number static
/// `BYTES_PER_ELEMENT` is never constructed under the test.
#[test]
fn typeof_member_guard_skips_program_class() {
    let decls = "class Typed { static BYTES_PER_ELEMENT = 4; constructor(a: any) {} }
        class Plain { constructor(a: any) { ext(svc); } }";
    assert!(!escapes(decls, "Math.random() < 2 ? Typed : Plain", "[1]", ""));
}

/// Control: a class whose static IS a Number passes the test; its
/// constructor runs and `svc` escapes.
#[test]
fn control_numeric_member_still_constructs() {
    let decls = "class Typed { static BYTES_PER_ELEMENT = 4; constructor(a: any) { ext(svc); } }";
    assert!(escapes(decls, "Typed", "[1]", ""));
}

/// Control: a host constructor may have any member; it is constructed with
/// `svc` and `svc` escapes.
#[test]
fn control_host_type_still_escapes() {
    assert!(escapes("", "Host", "svc", ""));
}

/// Control: a real thenable whose `then` is program code is still called by
/// the promise resolution (27.2.1.3.2 step 9-15): `svc` escapes through it.
#[test]
fn control_program_thenable_is_called() {
    let after = "const p: any = { then(res: any) { ext(svc); res(1); } }; Promise.resolve(p);";
    assert!(escapes("", "Float32Array", "[1]", after));
}

/// Precondition of the first test (passes today): without `Array` in `type`
/// nothing calls `obj.run`, so a failure there is the Array construct only.
#[test]
fn control_float32_only_modeled() {
    assert!(elem_mono("Float32Array"));
}

/// Precondition of the second test (passes today): `Typed` alone is modeled.
#[test]
fn control_typed_only_modeled() {
    let decls = "class Typed { static BYTES_PER_ELEMENT = 4; constructor(a: any) {} }
        class Plain { constructor(a: any) { ext(svc); } }";
    assert!(!escapes(decls, "Typed", "[1]", ""));
}

/// `svc` escapes from `convert([1], T)` with the guard body `body`
/// (`type` the second parameter).
fn conv_escapes(decls: &str, body: &str, ty: &str) -> bool {
    matches!(
        verdict_conv(true, decls, body, ty, "[1]", "", "size"),
        Verdict::No(_)
    )
}

const PLAIN: &str = "class Plain { constructor(a: any) { ext(svc); } }";
const TYPED: &str = "class Typed { static BYTES_PER_ELEMENT = 4; constructor(a: any) {} }";

/// (a) The read of the static is the real read: a static getter that
/// returns a Number passes the test (ECMA-262 10.1.8.1 step 5).
#[test]
fn g4_static_getter_number_is_admitted() {
    let decls = "class G { static get BYTES_PER_ELEMENT() { return 4; } constructor(a: any) { ext(svc); } }";
    assert!(escapes(decls, "G", "[1]", ""));
}

/// (a) A static inherited from the parent constructor (`Sub.[[Prototype]]`
/// is `Base`, 15.7.14 step 5) passes the test.
#[test]
fn g4_inherited_static_is_admitted() {
    let decls = "class Base { static BYTES_PER_ELEMENT = 4; }
        class Sub extends Base { constructor(a: any) { super(); ext(svc); } }";
    assert!(escapes(decls, "Sub", "[1]", ""));
}

/// (a) `Function.prototype` is add-locked in perry (the runtime refuses
/// every new key: `builtin_proto_tests.rs`
/// `the_add_locked_prototypes_refuse_every_new_key`), so a program store of
/// `BYTES_PER_ELEMENT` there adds nothing and `Array` still reads
/// `undefined`: the test stays false and no Array is constructed.
#[test]
fn g4_function_prototype_store_is_refused() {
    assert!(elem_mono_with(
        "(Function.prototype as any).BYTES_PER_ELEMENT = 4;",
        "Math.random() < 2 ? Float32Array : Array"
    ));
}

/// (a) A Number the program stores onto a parent constructor reaches the
/// subclass through its [[Prototype]] (`Sub.[[Prototype]]` is `Base`):
/// `Sub` passes the test.
#[test]
fn g4_static_stored_on_parent_is_admitted() {
    let decls = "class Base {} class Sub extends Base { constructor(a: any) { super(); ext(svc); } }
        (Base as any).BYTES_PER_ELEMENT = 4;";
    assert!(escapes(decls, "Sub", "[1]", ""));
}

/// (a) A static whose value is unknown (host) passes the test.
#[test]
fn g4_unknown_static_is_admitted() {
    let decls = "class H { constructor(a: any) { ext(svc); } } (H as any).BYTES_PER_ELEMENT = Host.n;";
    assert!(escapes(decls, "H", "[1]", ""));
}

/// The operand order does not matter (7.2.15 on two Strings).
#[test]
fn g4_reversed_operands_narrow() {
    let decls = &format!("{TYPED} {PLAIN}");
    let body = r#"if ("number" === typeof type.BYTES_PER_ELEMENT) return new type(array); return array;"#;
    assert!(!conv_escapes(decls, body, "Math.random() < 2 ? Typed : Plain"));
}

/// "undefined": a value whose read of the static is `undefined` (absent)
/// is kept and constructed.
#[test]
fn g4_undefined_keeps_only_absent() {
    let body = r#"if (typeof type.BYTES_PER_ELEMENT === "undefined") return new type(array); return array;"#;
    let decls2 = &format!("{TYPED} {PLAIN}");
    assert!(conv_escapes(decls2, body, "Math.random() < 2 ? Typed : Plain"));
}

/// "string": a String static passes, a Number static and an absent one do
/// not.
#[test]
fn g4_string_keeps_string_static() {
    let body = r#"if (typeof type.BYTES_PER_ELEMENT === "string") return new type(array); return array;"#;
    let decls = "class S { static BYTES_PER_ELEMENT = \"4\"; constructor(a: any) { ext(svc); } }
        class Q { constructor(a: any) {} }";
    assert!(conv_escapes(decls, body, "Math.random() < 2 ? S : Q"));
    let decls2 = &format!("{TYPED} {PLAIN}");
    assert!(!conv_escapes(decls2, body, "Math.random() < 2 ? Typed : Plain"));
}

/// (b) A write to `type` between the guard and the use: the new value is
/// constructed whatever the test said of the old one.
#[test]
fn g4_write_between_guard_and_use_keeps_new_value() {
    let decls = &format!("{TYPED} {PLAIN} const other: any = Plain;");
    let body = r#"if (typeof type.BYTES_PER_ELEMENT === "number") { type = other; return new type(array); } return array;"#;
    assert!(conv_escapes(decls, body, "Typed"));
}

/// (b) A guard on a member expression (`o.t.f`) narrows nothing.
#[test]
fn g4_member_host_narrows_nothing() {
    let decls = &format!("{TYPED} {PLAIN}");
    let body = r#"const o: any = { t: type }; if (typeof o.t.BYTES_PER_ELEMENT === "number") return new o.t(array); return array;"#;
    assert!(conv_escapes(decls, body, "Math.random() < 2 ? Typed : Plain"));
}

/// (c) The false side narrows nothing: `!==` true keeps `Plain`, whose
/// static is absent (so `!== "number"` holds for it).
#[test]
fn g4_false_side_keeps_plain() {
    let decls = &format!("{TYPED} {PLAIN}");
    let body = r#"if (typeof type.BYTES_PER_ELEMENT !== "number") return new type(array); return array;"#;
    assert!(conv_escapes(decls, body, "Math.random() < 2 ? Typed : Plain"));
}

/// (d) "object" is no G4 test: a `null` static (`typeof null` is
/// "object", 13.5.3) is constructed.
#[test]
fn g4_object_result_narrows_nothing() {
    let decls = "class N { static BYTES_PER_ELEMENT = null; constructor(a: any) { ext(svc); } }";
    let body = r#"if (typeof type.BYTES_PER_ELEMENT === "object") return new type(array); return array;"#;
    assert!(conv_escapes(decls, body, "N"));
}
