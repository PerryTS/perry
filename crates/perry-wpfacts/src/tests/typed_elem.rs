//! TYPED-ELEM (ECMA-262 10.4.5.16 TypedArraySetElement, 10.4.5.15
//! TypedArrayGetElement): an integer-indexed store into a typed array
//! stores ToNumber(v) (ToBigInt(v) on a BigInt64Array / BigUint64Array),
//! never `v`; a read of an element is that Number (BigInt) or undefined.
//! So an object stored there never comes back out of the element, while
//! the conversion's ToPrimitive still runs the program's `valueOf` with
//! the object as `this`. An Array (or a receiver that may be one) keeps
//! storing the value. `escapes`: node runs a method of `o`, so the facts
//! must not prove `g(o)`; a probe hands `r` (the element read) to unknown
//! code, so `r` holding the V instance makes `g(o)` unprovable.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class V {{ size = 3; m() {{ return 1; }} }}
        let hit: any = null;
        class W {{ size = 3; valueOf(): any {{ hit = this; return 1; }} }}
        class WB {{ size = 3; valueOf(): any {{ hit = this; return 1n; }} }}
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

// The fixture: a literal index, a value that may be the V instance.
case!(
    literal_index_element_holds_no_object,
    false,
    "const ta = new Float64Array(4); const o: any = Math.random() < 0.5 ? new V() : 1.5; ta[0] = o; const r = ta[0]; (globalThis as any).hostTake(r); console.log(g(o));"
);
// A computed Number index (the mixer: `buffer[offset]`).
case!(
    computed_index_element_holds_no_object,
    false,
    "const ta = new Float64Array(4); const o: any = Math.random() < 0.5 ? new V() : 1.5; const i = k * 2 + 1; ta[i] = o; const r = ta[i]; (globalThis as any).hostTake(r); console.log(g(o));"
);
// The mixer's shape (three.core.js 53332 / 53364): a field holding the
// typed array, a parameter offset, a value read from another object's field
// and written back into one.
case!(
    buffer_field_offset_parameter_holds_no_object,
    false,
    "class M { buffer: any = new Float64Array(8); put(t: any, name: string, off: number) { this.buffer[off] = t[name]; } back(t: any, name: string, off: number) { t[name] = this.buffer[off]; } } const o: any = new V(); const t: any = { p: o }; const q: any = {}; const mx = new M(); mx.put(t, 'p', k); mx.back(q, 'p', k + 1); (globalThis as any).hostTake(q.p); console.log(g(o));"
);
// (i) A plain Array keeps the value.
case!(
    array_element_holds_the_object,
    true,
    "const a: any = []; const o: any = Math.random() < 0.5 ? new V() : 1.5; a[0] = o; const r = a[0]; (globalThis as any).hostTake(r); console.log(g(o));"
);
// (ii) ToNumber runs W's valueOf with the W instance as `this`.
case!(
    store_runs_value_of_with_the_object_as_this,
    true,
    "const ta = new Float64Array(4); const o: any = new W(); ta[0] = o; (globalThis as any).hostTake(hit); console.log(g(o));"
);
case!(
    computed_store_runs_value_of_with_the_object_as_this,
    true,
    "const ta = new Float64Array(4); const o: any = new W(); ta[k] = o; (globalThis as any).hostTake(hit); console.log(g(o));"
);
// (iii) A receiver that may be a Float64Array or an Array: the Array side.
case!(
    typed_array_or_array_receiver_holds_the_object,
    true,
    "const b: any = Math.random() < 0.5 ? new Float64Array(4) : []; const o: any = new V(); b[0] = o; const r = b[0]; (globalThis as any).hostTake(r); console.log(g(o));"
);
// (iv) A BigInt64Array: the element is a BigInt (ToBigInt), never `o`.
case!(
    bigint_array_element_holds_no_object,
    false,
    "const bt = new BigInt64Array(2); const o: any = Math.random() < 0.5 ? new WB() : 1n; bt[0] = o; const r = bt[0]; (globalThis as any).hostTake(r); console.log(g(o));"
);
case!(
    bigint_array_store_runs_value_of,
    true,
    "const bt = new BigInt64Array(2); const o: any = new WB(); bt[k] = o; (globalThis as any).hostTake(hit); console.log(g(o));"
);
// A string-named property of a typed array is an ordinary property.
case!(
    named_property_holds_the_object,
    true,
    "const ta: any = new Float64Array(4); const o: any = new V(); ta.foo = o; (globalThis as any).hostTake(ta.foo); console.log(g(o));"
);
// 10.4.5.4 [[Get]]: a Number key is a CanonicalNumericIndexString, so the
// read is TypedArrayGetElement: never a property an unknown-String store
// wrote (that store stays an ordinary one: the solver has no fact for
// "canonical numeric string", so it is kept, coarser, not wrong).
case!(
    number_read_never_sees_a_string_keyed_store,
    false,
    "const ta: any = new Float64Array(4); const s = String(Math.random()); const o: any = new V(); ta[s] = o; (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
// The mixer's `targetObject[propertyName] = …` where the target may be the
// buffer itself.
case!(
    number_read_never_sees_a_store_through_a_maybe_buffer_target,
    false,
    "const ta: any = new Float64Array(4); const s = String(Math.random()); const tgt: any = k > 0 ? ta : {}; const o: any = new V(); tgt[s] = o; (globalThis as any).hostTake(ta[k * 2]); console.log(g(o));"
);
// The String-keyed read still sees the String-keyed store.
case!(
    string_read_sees_the_string_keyed_store,
    true,
    "const ta: any = new Float64Array(4); const s = String(Math.random()); const o: any = new V(); ta[s] = o; (globalThis as any).hostTake(ta[s]); console.log(g(o));"
);
// A key that may be a BigInt: ToString of a large BigInt is no canonical
// numeric string, an ordinary property (10.4.5.5): the read keeps it.
case!(
    bigint_key_read_keeps_the_ordinary_store,
    true,
    "const ta: any = new Float64Array(4); const s = String(Math.random()); const o: any = new V(); ta[s] = o; const i: any = k > 0 ? 2 : 123456789012345678901234567890n; (globalThis as any).hostTake(ta[i]); console.log(g(o));"
);
