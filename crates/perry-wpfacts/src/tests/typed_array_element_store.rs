//! ECMA-262 10.4.5.5 / 10.4.5.16: a TypedArray [[Set]] under a numeric key
//! runs TypedArraySetElement, which stores ToNumber(v) (ToBigInt(v) for the
//! BigInt arrays), never `v` itself; the ToPrimitive that conversion runs
//! (a user `valueOf`) stays active. An ordinary Array, a receiver that may
//! be one, and a non-numeric string key keep storing the value.
//! `escapes`: node runs a method of `o`, so the facts must not prove it.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        class ValueOf {{ size = 3; valueOf(): any {{ (globalThis as any).hostTake(this); return 1; }} }}
        function g(a: any) {{ return a.size; }}
        const k = Math.floor(Math.random() * 2);
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

case!(
    typed_array_element_is_a_number,
    false,
    "const ta: any = new Float64Array(2); const o: any = new Svc(); ta[k] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
case!(
    typed_array_literal_index_is_a_number,
    false,
    "const ta: any = new Float64Array(2); const o: any = new Svc(); ta[0] = o; (globalThis as any).hostTake(ta[0]); console.log(g(o));"
);
case!(
    array_element_is_the_value,
    true,
    "const a: any = new Array(2); const o: any = new Svc(); a[k] = o; (globalThis as any).hostTake(a[k]); console.log(g(o));"
);
case!(
    typed_array_element_store_runs_value_of,
    true,
    "const ta: any = new Float64Array(2); const o: any = new ValueOf(); ta[k] = o; console.log(g(o));"
);
case!(
    typed_array_or_array_receiver_keeps_the_array_side,
    true,
    "const b: any = [new Float64Array(2), []][k]; const o: any = new Svc(); b[k] = o; (globalThis as any).hostTake(b[k]); console.log(g(o));"
);
case!(
    typed_array_string_key_keeps_the_value,
    true,
    "const ta: any = new Float64Array(2); const s = k > 5 ? \"x\" : \"y\"; const o: any = new Svc(); ta[s] = o; (globalThis as any).hostTake(ta[s]); console.log(g(o));"
);
