//! ECMA-262 7.1.19 ToPropertyKey: a Number key is Number::toString, a
//! BigInt key BigInt::toString (6.1.6.2.23: an optional "-" and decimal
//! digits). On a typed array (10.4.5.5 [[Set]]) such a key names an
//! element (CanonicalNumericIndexString: TypedArraySetElement stores
//! ToNumber / ToBigInt of the value) or, for a BigInt whose digits are no
//! canonical Number string, an ordinary property named by digits only. It
//! never names `toJSON`, `foo` or any other non-digit name.
//! `escapes`: node runs a method of `o`, so the facts must not prove it.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.size; }}
        const k = Math.floor(Math.random() * 2);
        const w: any = k > 0 ? 3n : 2;
        {body}
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

// (a) A Number-or-BigInt key never names `toJSON`.
case!(
    bigint_key_names_no_to_json,
    false,
    "const ta: any = new Float64Array(4); const o: any = new Svc(); ta[w] = o; const j = ta.toJSON; (globalThis as any).hostTake(j); console.log(g(o));"
);
// (b) Nor any other non-digit name.
case!(
    bigint_key_names_no_foo,
    false,
    "const ta: any = new Float64Array(4); const o: any = new Svc(); ta[w] = o; (globalThis as any).hostTake(ta.foo); console.log(g(o));"
);
// (c) A digit name a BigInt may spell holds the raw value.
case!(
    bigint_key_digit_name_may_hold_the_value,
    true,
    "const ta: any = new Float64Array(4); const o: any = new Svc(); ta[w] = o; (globalThis as any).hostTake(ta[\"12345678901234567890\"]); console.log(g(o));"
);
// (c) So may an unknown String key.
case!(
    bigint_key_unknown_string_read_may_hold_the_value,
    true,
    "const ta: any = new Float64Array(4); const o: any = new Svc(); ta[w] = o; const s: any = (globalThis as any).hostKey(); (globalThis as any).hostTake(ta[s]); console.log(g(o));"
);
// (d) A key that may be any String may name `toJSON`.
case!(
    string_key_may_name_to_json,
    true,
    "const ta: any = new Float64Array(4); const o: any = new Svc(); const s: any = (globalThis as any).hostKey(); const x: any = k > 0 ? s : 2; ta[x] = o; (globalThis as any).hostTake(ta.toJSON); console.log(g(o));"
);
// (e) A plain object receiver keeps the any-name store.
case!(
    plain_object_receiver_keeps_any_name,
    true,
    "const ta: any = new Float64Array(4); const p: any = {}; const r: any = k > 0 ? ta : p; const o: any = new Svc(); r[w] = o; (globalThis as any).hostTake(p.toJSON); console.log(g(o));"
);
// (f) A Number key is an element store: the element is a Number.
case!(
    number_key_is_an_element_store,
    false,
    "const ta: any = new Float64Array(4); const o: any = new Svc(); ta[k] = o; (globalThis as any).hostTake(ta[k]); (globalThis as any).hostTake(ta.toJSON); console.log(g(o));"
);
