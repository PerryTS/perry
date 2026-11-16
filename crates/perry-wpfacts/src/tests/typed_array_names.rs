//! ECMA-262 10.4.5.4 / 10.4.5.5: on an integer-indexed exotic object (a
//! typed array) only a CanonicalNumericIndexString key names an element;
//! any other String key is an ordinary property (OrdinaryGet/OrdinarySet).
//! So a named non-index key never reads the elements: `ta.length` is the
//! %TypedArray%.prototype.length getter's Number (23.2.3.21), never an
//! element value. A named store and a named read still meet; a store under
//! a key that may be anything is seen by named reads and by element reads.
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

// (a) `ta.length` is a Number: the store is a Number-key store, which a
// named non-numeric key (`a`) never reads.
case!(
    typed_array_length_key_names_no_field,
    false,
    "const ta = new Float32Array(6); const h: any = { a: 1 }; const o: any = new Svc(); h[ta.length] = o; (globalThis as any).hostTake(h.a); console.log(g(o));"
);
// (b) a named store on a typed array is an ordinary property: the named
// read sees it.
case!(
    typed_array_named_store_is_read_by_name,
    true,
    "const ta: any = new Float32Array(6); const o: any = new Svc(); ta.foo = o; (globalThis as any).hostTake(ta.foo); console.log(g(o));"
);
// (c) a store under an unknown String key may be `foo`.
case!(
    typed_array_unknown_string_key_is_read_by_name,
    true,
    "const ta: any = new Float32Array(6); const s = String(Math.random()); const o: any = new Svc(); ta[s] = o; (globalThis as any).hostTake(ta.foo); console.log(g(o));"
);
// (d) a named store never reaches the elements.
case!(
    typed_array_named_store_is_no_element,
    false,
    "const ta: any = new Float32Array(6); const o: any = new Svc(); ta.foo = o; (globalThis as any).hostTake(ta[0]); (globalThis as any).hostTake(ta[k]); console.log(g(o));"
);
// (d') an unknown String key may be an index: the element store runs
// ToNumber(v), so `valueOf` runs.
case!(
    typed_array_unknown_string_key_may_be_an_index,
    true,
    "const ta: any = new Float32Array(6); const s = String(Math.random()); const o: any = new ValueOf(); ta[s] = o; (globalThis as any).hostTake(ta[0]); console.log(g(o));"
);
// (e) a key that may be anything: named and element reads both see it
// (the control stays conservative: the store is not split by key kind).
case!(
    typed_array_any_key_is_read_by_name,
    true,
    "const ta: any = new Float32Array(6); const key: any = (globalThis as any).hostKey(); const o: any = new Svc(); ta[key] = o; (globalThis as any).hostTake(ta.foo); console.log(g(o));"
);
case!(
    typed_array_any_key_is_read_by_index,
    true,
    "const ta: any = new Float32Array(6); const key: any = (globalThis as any).hostKey(); const o: any = new Svc(); ta[key] = o; (globalThis as any).hostTake(ta[0]); console.log(g(o));"
);
