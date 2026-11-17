//! ECMA-262 23.2.3.21: a typed array has no own `length`; `ta.length` finds
//! the %TypedArray%.prototype accessor, whose getter returns a Number. The
//! accessor has no setter, so a [[Set]] of `length` (a named store or one
//! under a key that may be "length") is OrdinarySet (10.4.5.5, 10.1.9.2)
//! returning false: it makes no own property. Only [[DefineOwnProperty]]
//! makes an own `length`, and only a changed prototype chain or a
//! redefined prototype accessor answers something else.
//! `escapes`: node runs a method of `o`, so the facts must not prove it.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.size; }}
        const c = Math.random() < 0.5;
        const ta: any = new Float32Array(6);
        const h: any = {{ a: 1 }};
        const o: any = new Svc();
        {body}
        h[ta.length] = o;
        (globalThis as any).hostTake(h.a);
        console.log(g(o));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

macro_rules! case {
    ($name:ident, $escapes:expr, $body:expr) => {
        #[test]
        fn $name() {
            let v = verdict($body);
            assert_eq!(!proven(&v), $escapes, "{v:?}");
        }
    };
}

// Open under the runtime lock: a BufferHeader view (a Node Buffer, a plain
// Uint8Array) takes a [[Set]] of `length` as an own data property
// (perry-runtime buffer/own_props.rs `buffer_set_own_prop`, reached from
// object/field_set_by_name.rs and object/polymorphic_index.rs without the
// inherited accessor), and the model's typed-array template does not keep
// its kind. The assertion is ECMA-262's answer for a Float32Array.
macro_rules! lock_case {
    ($name:ident, $body:expr) => {
        #[test]
        #[ignore = "runtime lock: a BufferHeader view makes an own length on [[Set]]; kind not tracked"]
        fn $name() {
            let v = verdict($body);
            assert!(proven(&v), "{v:?}");
        }
    };
}

// (a) a BigInt stored under a key that may be "x" or 3 is an element or an
// ordinary property `x`; never an own `length`.
case!(
    typed_array_length_after_any_key_bigint_store,
    false,
    r#"const kk: any = c ? "x" : 3; ta[kk] = (1n as any);"#
);
// (a2) the same under a key nothing names (unknown code returns it).
lock_case!(
    typed_array_length_after_unknown_key_bigint_store,
    r#"const kk: any = (globalThis as any).hostKey(); ta[kk] = (1n as any);"#
);
// (b) the same with the view's buffer exposed (B1): another view of the
// buffer writes elements only.
// Open: every HARD mark escapes the view (`mark_hard` -> `mark_escape_why`),
// B1 included, so rule C1 (not escaped) keeps the own field there: C2.
#[test]
#[ignore = "C2: a B1 HARD mark escapes the view itself"]
fn typed_array_length_with_buffer_exposed() {
    let v = verdict(
        r#"const kk: any = (globalThis as any).hostKey(); ta[kk] = (1n as any); (globalThis as any).hostKeep(ta.buffer);"#,
    );
    assert!(proven(&v), "{v:?}");
}
// (c) control: an own `length` defined by [[DefineOwnProperty]] is read.
case!(
    typed_array_length_defined_own_is_read,
    true,
    r#"Object.defineProperty(ta, "length", { value: 1n });"#
);
// (d) control: a replaced prototype answers its own `length`.
case!(
    typed_array_length_prototype_replaced,
    true,
    r#"Object.setPrototypeOf(ta, { length: 1n });"#
);
// (e) a plain [[Set]] of `length` fails (no setter): still the Number.
lock_case!(
    typed_array_length_plain_set_fails,
    r#"try { ta.length = (5n as any); } catch (e) {}"#
);
