//! ECMA-262 13.15.3 ApplyStringOrNumericBinaryOperator (and 13.8.1 for
//! `+`): after ToNumeric a BigInt result needs a BigInt on BOTH sides; a
//! BigInt beside a Number throws a TypeError. So `n * w`, `w + n` with `n`
//! a Number is a Number even when `w` may be a BigInt, and a typed array
//! store under that key is an element store (ToNumber(v), never `v`).
//! Two operands that may both be BigInts, and unary `-`, keep the BigInt.
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

case!(
    number_times_maybe_bigint_is_a_number,
    false,
    "const ta: any = new Float64Array(8); const o: any = new Svc(); ta[1 * w + k] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
case!(
    maybe_bigint_plus_number_is_a_number,
    false,
    "const ta: any = new Float64Array(8); const o: any = new Svc(); ta[w + k] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
case!(
    field_times_field_index_is_a_number,
    false,
    "class M { valueSize: any; buffer: any; _addIndex = 4; constructor(vs: any) { this.valueSize = vs; this.buffer = new Float64Array(16); } put(o: any) { const t = this._addIndex * this.valueSize; for (let i = 0; i < 2; i++) { this.buffer[t + i] = o; } } } const m = new M(w); const o: any = new Svc(); m.put(o); (globalThis as any).hostTake(m.buffer[k]); console.log(g(o));"
);
case!(
    maybe_bigint_times_maybe_bigint_keeps_the_bigint,
    true,
    "const ta: any = new Float64Array(8); const o: any = new Svc(); ta[w * w] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
case!(
    maybe_bigint_plus_maybe_bigint_keeps_the_bigint,
    true,
    "const ta: any = new Float64Array(8); const o: any = new Svc(); ta[w + w] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
case!(
    negated_maybe_bigint_keeps_the_bigint,
    true,
    "const ta: any = new Float64Array(8); const o: any = new Svc(); ta[-w] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
