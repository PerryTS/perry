//! OPAQUE2: builtin models whose result ECMA-262 fixes, where the model gave
//! a container of any primitive (or ⊤): DataView get/set (25.3.4),
//! `ArrayBuffer.isView` (25.1.5.1), the global functions (19.2), the
//! primitive constructors called as functions (20.3.1.1, 21.1.1.1, 21.2.1.1,
//! 22.1.1.1, 20.4.1.1), the Number/String statics (21.1.2, 22.1.2), the
//! own-keys statics (20.1.2.10/11/18), and copies of the own enumerable
//! properties of realm objects that have none at birth (spread,
//! `Object.assign`).
//! A member read on a result of one primitive kind resolves to that kind's
//! prototype member (`NativeMember`, a String's `OwnLength`); a result that
//! may be any primitive, or a primitive container, does not. Controls: a
//! key the kind lacks stays unproven, a host value stays unproven, a
//! conversion the specification names still calls the program's
//! `valueOf`/`toString`, a program store on a realm object is still copied.
use super::*;

const HEAD: &str = "function pick(x: any) { return x; }\n";

/// The verdict of `r.key` with `r = expr` (`dv` a DataView, `s` a String).
fn verdict_of(expr: &str, key: &str) -> String {
    let src = format!(
        "{HEAD}function f(dv: any, s: any) {{ const r: any = {expr}; return r.{key}; }}\nconsole.log(f(new DataView(new ArrayBuffer(16)), \"1\"));\n"
    );
    let pf = facts(&src);
    let at = src.find("const r").expect("marker") as u32;
    pf.sites
        .iter()
        .filter(|x| x.key == key && x.byte_offset >= at)
        .min_by_key(|x| x.byte_offset)
        .map(|x| format!("{:?}", x.verdict))
        .unwrap_or_else(|| "none".into())
}

/// Each `(expr, key, proven)`: is `r.key` proven?
fn reads(cases: &[(&str, &str, bool)]) {
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|&(e, k, want)| {
            let v = verdict_of(e, k);
            (v.starts_with("No(") == want).then(|| format!("want proven={want}: ({e}).{k} => {v}"))
        })
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

fn escapes(body: &str) -> bool {
    let src = format!(
        r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; }}
        function g(a: any) {{ return a.marker; }}
        {HEAD}
        const o: any = new Svc();
        const leak = function () {{ (globalThis as any).hostTake(o); return 1; }};
        const w: any = {{ valueOf: leak, toString: leak }};
        {body}
        console.log(g(o));
        "#
    );
    matches!(site(&facts(&src), "marker").verdict, Verdict::No(_))
}

fn check(cases: &[(String, bool)]) {
    let bad: Vec<String> = cases
        .iter()
        .filter(|(b, want)| escapes(b) != *want)
        .map(|(b, want)| format!("want escape={want}: {b}"))
        .collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// #2: DataView get* is a Number (a BigInt for the 64-bit ones), set* is
/// undefined (25.3.4.5-22); arguments get ToIndex / ToNumber / ToBigInt.
#[test]
fn dataview_get_set_results() {
    reads(&[
        ("dv.getUint32(0, true)", "toFixed", true),
        ("dv.getFloat64(0)", "valueOf", true),
        ("dv.getInt8(0)", "length", false),
        ("dv.getBigInt64(0)", "toFixed", false),
    ]);
    check(&[
        ("new DataView(new ArrayBuffer(8)).getUint8(w);".into(), true),
        ("new DataView(new ArrayBuffer(8)).setFloat32(0, w);".into(), true),
        ("(globalThis as any).hostTake(new DataView(new ArrayBuffer(8)).setUint8(0, 1));".into(), false),
    ]);
}

/// #3: `ArrayBuffer.isView(a)` is a Boolean; it reads one internal slot.
#[test]
fn array_buffer_is_view() {
    reads(&[("ArrayBuffer.isView(dv)", "toFixed", false)]);
    check(&[
        ("ArrayBuffer.isView(o);".into(), false),
        ("pick(ArrayBuffer.isView)(o);".into(), false),
        ("const r: any = pick(ArrayBuffer.isView)(1); try { r.foo(o); } catch (e) {}".into(), false),
        ("ArrayBuffer.isView(w);".into(), false),
    ]);
}

/// #4: the global functions (19.2) called as values: a fixed primitive per
/// name; the argument gets the conversion the specification names.
#[test]
fn global_functions() {
    reads(&[
        ("pick(parseInt)(s)", "toFixed", true),
        ("(globalThis as any).parseFloat(s)", "toFixed", true),
        ("pick(encodeURIComponent)(s)", "length", true),
        ("encodeURIComponent(s)", "length", true),
        ("decodeURI(s)", "length", true),
        ("atob(s)", "length", true),
        ("btoa(s)", "length", true),
        ("pick(isFinite)(s)", "toFixed", false),
        ("(globalThis as any).hostVal", "valueOf", false),
    ]);
    check(&[
        ("pick(parseInt)(w);".into(), true),
        ("pick(isNaN)(w);".into(), true),
        ("pick(encodeURI)(w);".into(), true),
        ("atob(w);".into(), true),
    ]);
}

/// #5: `String(x)`, `Number(x)`, `Boolean(x)`, `BigInt(x)`, `Symbol(d)`
/// called as function values; `Boolean` is ToBoolean (no ToPrimitive).
#[test]
fn primitive_constructors_called() {
    reads(&[
        ("pick(String)(s)", "length", true),
        ("pick(Number)(s)", "toFixed", true),
        ("pick(Number)(s)", "length", false),
    ]);
    check(&[
        ("pick(Boolean)(w);".into(), false),
        ("pick(Number)(w);".into(), true),
        ("pick(String)(w);".into(), true),
        ("Symbol(w);".into(), true),
    ]);
}

/// #6: `Number.is*` test the type and coerce nothing (21.1.2.2-5);
/// `String.fromCharCode` / `String.raw` are Strings (22.1.2.1, 22.1.2.4).
#[test]
fn number_and_string_statics() {
    reads(&[
        ("pick(Number.parseInt)(s)", "toFixed", true),
        ("String.fromCharCode(65)", "length", true),
        ("pick(String.fromCharCode)(65)", "length", true),
        ("String.raw({ raw: [s] } as any)", "length", true),
        ("String.fromCharCode(65)", "toFixed", false),
    ]);
    check(&[
        ("const r: any = pick(Number.isInteger)(1); try { r.foo(o); } catch (e) {}".into(), false),
        ("pick(Number.isInteger)(w); pick(Number.isSafeInteger)(w);".into(), false),
        ("pick(Number.isFinite)(w); pick(Number.isNaN)(w);".into(), false),
        ("pick(Number.parseFloat)(w);".into(), true),
        ("String.fromCharCode(w);".into(), true),
        ("String.raw({ raw: [\"a\", \"b\"] } as any, w);".into(), true),
    ]);
}

/// #9: `Object.keys` / `getOwnPropertyNames` hold Strings only,
/// `getOwnPropertySymbols` Symbols only.
#[test]
fn own_keys_element_types() {
    reads(&[
        ("Object.keys(dv)[0]", "length", true),
        ("pick(Object.keys)(dv)[0]", "length", true),
        ("Object.getOwnPropertyNames(dv)[0]", "length", true),
        ("Object.getOwnPropertySymbols(dv)[0]", "length", false),
        ("Reflect.ownKeys(dv)[0]", "length", false),
    ]);
}

/// #7: a copy of the own enumerable properties of a realm object born with
/// none reads only what the program stored on it.
#[test]
fn realm_objects_without_enumerable_own_props() {
    let kinds = [
        "new Map()",
        "new Set()",
        "new WeakMap()",
        "new WeakSet()",
        "new Date()",
        "new ArrayBuffer(8)",
        "new DataView(new ArrayBuffer(8))",
    ];
    let mut cases = Vec::new();
    for k in kinds {
        cases.push((format!("const x: any = {k}; const c: any = {{ ...x }}; try {{ c.foo(o); }} catch (e) {{}}"), false));
        cases.push((format!("const x: any = {k}; const c: any = Object.assign({{}}, x); try {{ c.foo(o); }} catch (e) {{}}"), false));
        cases.push((format!("const x: any = {k}; x.foo = (y: any) => (globalThis as any).hostTake(y); const c: any = {{ ...x }}; c.foo(o);"), true));
    }
    check(&cases);
}
