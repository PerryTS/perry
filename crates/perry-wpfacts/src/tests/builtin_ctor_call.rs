//! CTORCALL: a builtin constructor the specification gives no [[Call]]
//! behaviour but a TypeError throws before it uses any argument when it is
//! called without `new` (NewTarget undefined): %TypedArray% and the concrete
//! typed-array constructors (ECMA-262 23.2.1.1, 23.2.5.1 step 1), ArrayBuffer
//! (25.1.4.1), SharedArrayBuffer (25.2.3.1), DataView (25.3.2.1), Map
//! (24.1.1.1), Set (24.2.1.1), WeakMap (24.3.1.1), WeakSet (24.4.1.1),
//! Promise (27.2.3.1). The runtime: `object/global_this/ctor_thunks.rs`
//! (`typed_array_constructor_call_thunk`, `construct_only_builtin_call_thunk`,
//! `map_`/`set_`/`weak_map_`/`weak_set_`/`promise_constructor_call_thunk`),
//! the code pointer of each of these globals (`global_this_tables.rs`). The
//! call hands its receiver and arguments to no code. three.js skin: an
//! any-key read on a typed array holds `constructor`; stored into a JSON
//! object's any-key, it is read back as `then` of a promise's fulfilment
//! value and called with the resolving functions (27.2.1.3.2 step 13, 27.2.2.2
//! NewPromiseResolveThenableJob): that call throws, the promise rejects.
//! Controls: `new` constructs as before; a program `then` is still called
//! with the resolving functions; a host `then` still receives them;
//! constructors that are callable without `new` keep their model.
//! `escapes`: the facts must not prove `o.marker` iff unknown code may hold `o`.
use super::*;

fn escapes(body: &str) -> bool {
    let src = format!(
        r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; }}
        function g(a: any) {{ return a.marker; }}
        function pick(x: any) {{ return x; }}
        const o: any = new Svc();
        {body}
        console.log(g(o));
        "#
    );
    matches!(site(&facts(&src), "marker").verdict, Verdict::No(_))
}

const NEW_ONLY: &[&str] = &[
    "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array",
    "Uint32Array", "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array",
    "ArrayBuffer", "SharedArrayBuffer", "DataView", "Map", "Set", "WeakMap", "WeakSet", "Promise",
];

fn plain_calls(k: &str) -> Vec<String> {
    vec![
        format!("const w: any = {{ v: o }}; const f: any = pick({k}); try {{ f(w); }} catch (e) {{}}"),
        format!("const f: any = pick({k}); try {{ f(o); }} catch (e) {{}}"),
        format!("const h: any = {{ k: pick({k}) }}; try {{ h.k(o); }} catch (e) {{}}"),
        format!("const f: any = pick({k}); try {{ f.call(o, o); }} catch (e) {{}}"),
        format!("const f: any = pick({k}); try {{ f.apply(o, [o]); }} catch (e) {{}}"),
    ]
}

/// (i) A plain call of each new-only constructor: nothing escapes.
#[test]
fn a_new_only_constructor_called_without_new_uses_no_argument() {
    let bad: Vec<String> = NEW_ONLY
        .iter()
        .flat_map(|k| plain_calls(k))
        .filter(|b| escapes(b))
        .collect();
    assert!(bad.is_empty(), "escaped through a builtin constructor [[Call]]: {bad:#?}");
}

fn thenables(k: &str) -> Vec<String> {
    let t = format!("const t: any = {{ then: pick({k}), v: o }};");
    vec![
        format!("{t} Promise.resolve(t).then((x: any) => (globalThis as any).hostTake(x), (e: any) => 0);"),
        format!("{t} Promise.resolve(1).then(() => t).then((x: any) => (globalThis as any).hostTake(x), (e: any) => 0);"),
        format!("{t} Promise.all([t, 1]).then((x: any) => (globalThis as any).hostTake(x), (e: any) => 0);"),
        format!("{t} new Promise((r: any) => r(t)).then((x: any) => (globalThis as any).hostTake(x), (e: any) => 0);"),
        format!("{t} (async () => {{ try {{ await t; }} catch (e) {{}} }})();"),
    ]
}

/// (ii) The skin shape: a new-only constructor as `then` of a fulfilment
/// value: the thenable job's call throws, nothing escapes.
#[test]
fn a_new_only_constructor_as_then_rejects_and_uses_nothing() {
    let bad: Vec<String> = ["Int8Array", "Float32Array", "ArrayBuffer", "DataView", "Map", "WeakSet", "Promise"]
        .iter()
        .flat_map(|k| thenables(k))
        .filter(|b| escapes(b))
        .collect();
    assert!(bad.is_empty(), "escaped through a builtin constructor as then: {bad:#?}");
}

/// (iii) Control: `new` of the same value constructs as before (an
/// argument the construction stores is a value of the result).
#[test]
fn new_on_the_constructor_value_still_constructs() {
    for b in [
        "const f: any = pick(Map); const m = new f([[1, o]]); (globalThis as any).hostTake(m);",
        "const f: any = pick(Set); const s = new f([o]); (globalThis as any).hostTake(s);",
        "const f: any = pick(Promise); const p = new f((r: any) => r(o)); p.then((x: any) => (globalThis as any).hostTake(x));",
        "const f: any = pick(WeakMap); const m = new f([[o, 1]]); (globalThis as any).hostTake(m);",
    ] {
        assert!(escapes(b), "{b}");
    }
}

/// (iv) Control: a program `then` is still called with the resolving
/// functions; a host `then` still receives them and the thenable.
#[test]
fn a_program_or_host_then_is_still_called() {
    let b = "const t: any = { then(res: any) { res(o); } }; Promise.resolve(t).then((x: any) => (globalThis as any).hostTake(x));";
    assert!(escapes(b), "{b}");
    let b = "const t: any = { then: (globalThis as any).hostGet(), v: o }; Promise.resolve(t);";
    assert!(escapes(b), "{b}");
}

/// (v) Control: an unknown value beside the constructor keeps the unknown call.
#[test]
fn an_unknown_value_beside_the_constructor_keeps_the_unknown_call() {
    for b in plain_calls("(Math.random() < 2 ? Int8Array : (globalThis as any).hostGet())").iter().skip(1) {
        assert!(escapes(b), "{b}");
    }
}

/// (vi) Control: constructors callable without `new` keep today's model:
/// each answer printed red (before the rule) is the answer after it.
#[test]
fn callable_constructors_keep_their_model() {
    let got: Vec<(String, bool)> = [
        "const a: any = Array(o); (globalThis as any).hostTake(a);",
        "const a: any = Object(o); (globalThis as any).hostTake(a);",
        "String(o);",
        "Number(o);",
        "Boolean(o);",
        "Date();",
        "RegExp(\"a\");",
        "Error(\"m\");",
        "const a: any = Array(o); console.log(a.length);",
    ]
    .iter()
    .map(|b| (b.to_string(), escapes(b)))
    .collect();
    let want = [true, true, false, false, false, false, false, false, false];
    let bad: Vec<_> = got.iter().zip(want).filter(|((_, g), w)| g != w).collect();
    assert!(bad.is_empty(), "changed: {bad:#?} (all: {got:#?})");
}
