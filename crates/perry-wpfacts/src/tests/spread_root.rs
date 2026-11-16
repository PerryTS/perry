//! TRACE8769: `Object.assign(t, src)` / `{...src}` copies src's own enumerable
//! properties (ECMA-262 20.1.2.1, 7.3.25 CopyDataProperties). A TypedArray's own
//! keys are its integer indices, then the keys the program gave it (10.4.5.7
//! [[OwnPropertyKeys]]); an index's value is a Number or a BigInt (10.4.5.1,
//! 10.4.5.15 TypedArrayGetElement). A builtin Array the model made holds the
//! elements the model gave it. Neither copies a key the model does not know, so a
//! spread of one puts no ⊤ into the target's any-key field. The target is then
//! shown by console.log (the runtime formatter looks up the inspect hook,
//! `__perry_inspect_custom__`): with ⊤ there it is an unknown call whose receiver
//! is the target, and `svc` stored in the target escapes.
use super::*;

fn escapes(src_expr: &str, copy: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        const svc: any = new Svc();
        class Node3 {{ userData: any = {{}}; name = "n"; }}
        const node = new Node3();
        node.userData.s = svc;
        const src: any = {src_expr};
        {copy}
        console.log(node);
        function g(a: any) {{ return a.size; }}
        console.log(g(svc));
        "#
    );
    let v = site(&facts(&src), "size").verdict.clone();
    matches!(v, Verdict::No(_))
}

const ASSIGN: &str = "Object.assign(node.userData, src);";
const SPREAD: &str = "node.userData = { ...src, s: svc };";

/// Object.assign from a Float32Array: index keys with Number values only.
#[test]
fn assign_from_typed_array_keeps_target_modeled() {
    assert!(!escapes("new Float32Array(4)", ASSIGN));
}

/// `{...ta}`: the same copy (CopyDataProperties).
#[test]
fn spread_of_typed_array_keeps_target_modeled() {
    assert!(!escapes("new Float32Array(4)", SPREAD));
}

/// A builtin Array the model made (`new Array(x)` with one non-number
/// argument: [x], 23.1.1.1): its elements are known.
#[test]
fn assign_from_builtin_array_keeps_target_modeled() {
    assert!(!escapes("new Array(Math.random() < 2 ? { a: 1 } : 0)", ASSIGN));
}

/// Control: a plain literal source is modeled today.
#[test]
fn control_literal_source_modeled() {
    assert!(!escapes("{ a: 1 }", ASSIGN));
}

/// Control: a host value may have any own key, a Symbol-keyed inspect hook too:
/// the target's inspect hook is unknown and `svc` escapes.
#[test]
fn control_host_source_escapes() {
    assert!(escapes("(globalThis as any).hostGive()", ASSIGN));
}

/// The three.js shape (`convertArray`: `new type(array)` with `type` Array or a
/// typed-array constructor): the solve-time Array born at that call
/// (`Array@call`, a `BObj "Array"`) holds the elements the model gave it.
#[test]
fn assign_from_dynamic_ctor_result_keeps_target_modeled() {
    assert!(!escapes(
        "new (Math.random() < 0.5 ? Array : Float32Array as any)({ a: 1 })",
        ASSIGN
    ));
}

/// `svc` escapes iff the copy carried it into `t` (`hostTake` is unknown code).
fn copy_escapes(body: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        const svc: any = new Svc();
        const t: any = {{}};
        {body}
        function g(a: any) {{ return a.size; }}
        console.log(g(svc));
        "#
    );
    let v = site(&facts(&src), "size").verdict.clone();
    matches!(v, Verdict::No(_))
}

/// A key the program gave a typed array is an ordinary own enumerable
/// property (10.4.5.7): the copy stores it on the target, and unknown code
/// given the target's `foo` holds `svc`. Can fail: drops `foo` → modeled.
#[test]
fn assign_from_typed_array_copies_expando() {
    assert!(copy_escapes(
        "const ta: any = new Float32Array(2); ta.foo = svc; Object.assign(t, ta); (globalThis as any).hostTake(t.foo);"
    ));
}

/// Without the hand-off the same copy keeps `svc` modeled.
#[test]
fn assign_from_typed_array_expando_unhanded_modeled() {
    assert!(!copy_escapes(
        "const ta: any = new Float32Array(2); ta.foo = svc; Object.assign(t, ta); console.log(t.foo === 1);"
    ));
}

/// An Array's elements are own enumerable index keys: `{...[svc]}` holds
/// `svc` under "0", and unknown code given `t[0]` holds it.
#[test]
fn spread_of_object_array_copies_elements() {
    assert!(copy_escapes(
        "const arr: any[] = [svc]; const u: any = { ...arr }; (globalThis as any).hostTake(u[0]);"
    ));
}
