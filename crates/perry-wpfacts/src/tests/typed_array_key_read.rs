//! TAKEY: a computed read `ta[k]` on a typed array (or a Buffer) is
//! ECMA-262 10.4.5.4 [[Get]] of a TypedArray exotic object: a Number key or a
//! canonical numeric String reads TypedArrayGetElement (10.4.5.15: the element
//! kind, Number or BigInt, or `undefined`); any other key is OrdinaryGet
//! (10.1.8.1): the properties the program stored on the array, then the
//! members of its prototype chain (`%TypedArray%.prototype`, the concrete
//! prototype, `Object.prototype`) or `undefined`. A key of unknown string reads
//! the union: named values and primitive kinds, never an unknown value, unless
//! the receiver is unknown or a store into an unknown value may have written
//! the chain.
//! `escapes`: the facts must not prove `o.size` iff unknown code may hold `o`.
use super::*;

fn escapes(body: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.size; }}
        const k: any = process.argv[2];
        const o: any = new Svc();
        {body}
        console.log(g(o));
        "#
    );
    matches!(site(&facts(&src), "size").verdict, Verdict::No(_))
}

macro_rules! case {
    ($name:ident, $escapes:expr, $body:expr) => {
        #[test]
        fn $name() {
            assert_eq!(escapes($body), $escapes, "{}", $body);
        }
    };
}

// A constant key naming a data property of the chain reads its kind:
// `BYTES_PER_ELEMENT` (23.2.7.2) is a Number, no function to call.
case!(
    bytes_per_element_is_a_number,
    false,
    "const ta: any = new Float32Array(4); const r: any = ta[\"BYTES_PER_ELEMENT\"]; r(o);"
);
// The skin shape (three.js PropertyBinding: `target[name] = ta[k]`, then a
// store through the result): the result is no unknown value, so a store into
// it does not hand `o` to unknown code.
case!(
    store_into_unknown_key_read_keeps_value_modeled,
    false,
    "const ta: any = new Float32Array(4); const t: any = ta[k]; t[k] = o;"
);
case!(
    unknown_key_read_stored_into_object_keeps_object_modeled,
    false,
    "const ta: any = new Float32Array(4); const tgt: any = { v: o }; tgt[k] = ta[k]; tgt.v2 = tgt[k]; tgt.v2.w = o;"
);
// A Number key: an element (Number) or `undefined`.
case!(
    number_key_reads_element_or_undefined,
    false,
    "const ta: any = new Float32Array(4); const i = Math.floor(Math.random() * 9); const t: any = ta[i]; t[k] = o; ta[i](o);"
);
// Constant keys: `length` a Number, `slice` the named native, `nope` undefined.
case!(
    const_length_key_reads_number,
    false,
    "const ta: any = new Float32Array(4); const t: any = ta[\"length\"]; t[k] = o;"
);
case!(
    const_slice_key_reads_named_native,
    false,
    "const ta: any = new Float32Array(4); const t: any = ta[\"slice\"]; t[k] = o;"
);
case!(
    const_absent_key_reads_undefined,
    false,
    "const ta: any = new Float32Array(4); const t: any = ta[\"nope\"]; t[k] = o;"
);
// BigInt kind: elements are BigInts.
case!(
    bigint_array_unknown_key_read_is_no_unknown_value,
    false,
    "const ta: any = new BigInt64Array(4); const t: any = ta[k]; t[k] = o;"
);
// Control (other direction): a property the program stored is read.
case!(
    expando_is_read_by_unknown_key,
    true,
    "const ta: any = new Float32Array(4); ta.foo = o; const r: any = ta[k]; r.leak();"
);
// Control: an unknown receiver stays unknown.
case!(
    control_host_receiver_stays_unknown,
    true,
    "const ta: any = (globalThis as any).hostGive(); const r: any = ta[k]; r(o);"
);
// Control: a value unknown code wrote onto the chain is read.
case!(
    control_written_chain_is_read,
    true,
    "(Object.prototype as any).zz = (globalThis as any).hostGive(); const ta: any = new Float32Array(4); const r: any = ta[k]; r(o);"
);
// `@@toStringTag` (23.2.3.38): the typed array's name, a String.
case!(
    to_string_tag_reads_a_string,
    false,
    "const ta: any = new Float32Array(4); const t: any = ta[Symbol.toStringTag]; t[k] = o;"
);
// A Buffer (a Uint8Array): the same read.
case!(
    buffer_unknown_key_read_is_no_unknown_value,
    false,
    "const ta: any = Buffer.alloc(4); const t: any = ta[k]; t[k] = o;"
);
// Node's `parent` / `offset` getters on a Buffer: its ArrayBuffer and a
// Number, named or under an unknown key.
case!(
    buffer_parent_and_offset_named_reads,
    false,
    "const b: any = Buffer.alloc(4); const t: any = b.parent; t[k] = o; const u: any = b.offset; u[k] = o;"
);
