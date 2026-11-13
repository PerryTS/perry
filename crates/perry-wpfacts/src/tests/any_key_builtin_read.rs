//! ABKEY: a computed read `o[k]` on a builtin object is the same [[Get]] as
//! the named read `o.name` (ECMA-262 13.3.2.1 EvaluatePropertyAccessWith-
//! ExpressionKey, 7.3.2 Get): under a key of unknown string it reads the
//! union of the named reads of the keys the computed key may spell, its own
//! properties, and what a store into an unknown value may have written onto
//! its chain. An accessor reads its declared result: `ArrayBuffer.prototype`
//! `byteLength`/`maxByteLength` Numbers, `resizable`/`detached` Booleans
//! (25.1.6), `SharedArrayBuffer.prototype` `byteLength`/`maxByteLength`
//! Numbers, `growable` a Boolean (25.2.5), `DataView.prototype` `buffer` the
//! viewed buffer, `byteLength`/`byteOffset` Numbers (25.3.4); each
//! `@@toStringTag` a String. three.js `targetObject = targetObject[name]`
//! (PropertyBinding) reads again whatever an unknown-key read returned: the
//! read must be exact on every builtin value that can come out.
//! `escapes`: the facts must not prove `o.size` iff unknown code may hold `o`.
use super::*;

fn escapes(body: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.size; }}
        const k: any = process.argv[2];
        const k2: any = process.argv[3];
        const keys: any[] = process.argv.slice(4);
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

/// Does `x` (stored into the program object `holder.xv`) hold ⊤ after
/// `body`? The value the reads return, apart from what a later store into
/// it does (a store under an unknown key into a builtin function or
/// constructor is the store rule's, E6).
fn holds_top(body: &str) -> bool {
    let src = format!(
        r#"
        const k: any = process.argv[2];
        const k2: any = process.argv[3];
        const keys: any[] = process.argv.slice(4);
        const holder: any = {{ xv: 0 }};
        {body}
        holder.xv = x;
        console.log(holder.xv);
        "#
    );
    let pf = facts(&src);
    let fs: Vec<_> = pf
        .templates
        .iter()
        .flat_map(|t| &t.fields)
        .filter(|f| f.key == "xv")
        .collect();
    assert!(!fs.is_empty(), "no field xv");
    fs.iter().any(|f| f.kind.top)
}

macro_rules! top_case {
    ($name:ident, $top:expr, $body:expr) => {
        #[test]
        fn $name() {
            assert_eq!(holds_top($body), $top, "{}", $body);
        }
    };
}

// The skin shape (three.js PropertyBinding): two unknown-key reads, the
// second on what the first returned (through `ta.buffer`, an ArrayBuffer),
// then the result is stored into an object under an unknown key.
top_case!(
    skin_second_read_holds_no_unknown_value,
    false,
    "const t: any = new Float32Array(4); let x: any = t[k]; x = x[k2];"
);
case!(
    skin_target_stays_modeled,
    false,
    "const t: any = new Float32Array(4); let x: any = t[k]; x = x[k2]; const target: any = { v: o }; target[k] = x; target.v2 = target[k2]; target.v2.w = o;"
);
// The closure: whatever an unknown-key read returns is read again.
top_case!(
    closure_from_typed_array,
    false,
    "let x: any = new Float32Array(4); for (let i = 0; i < keys.length; i++) x = x[keys[i]];"
);
top_case!(
    closure_from_data_view,
    false,
    "let x: any = new DataView(new ArrayBuffer(8)); for (let i = 0; i < keys.length; i++) x = x[keys[i]];"
);
top_case!(
    closure_from_array_buffer,
    false,
    "let x: any = new ArrayBuffer(8); for (let i = 0; i < keys.length; i++) x = x[keys[i]];"
);
// Part C, one receiver kind each: a String, a Number, a Boolean (their
// wrapper prototypes' members), a named native function (`name`, `length`,
// `Function.prototype`'s members), an Array.
case!(
    string_unknown_key_read,
    false,
    "const s: any = \"abc\"; const r: any = s[k]; r[k2] = o;"
);
case!(
    number_unknown_key_read,
    false,
    "const n: any = 5; const r: any = n[k]; r[k2] = o;"
);
case!(
    boolean_unknown_key_read,
    false,
    "const b: any = true; const r: any = b[k]; r[k2] = o;"
);
case!(
    named_native_function_unknown_key_read,
    false,
    "const f: any = Math.max; const r: any = f[k]; r[k2] = o;"
);
case!(
    array_unknown_key_read,
    false,
    "const a: any = [1, 2]; const r: any = a[k]; r[k2] = o;"
);
// Named reads of the buffer kinds' accessors.
case!(
    array_buffer_max_byte_length_is_a_number,
    false,
    "const ab: any = new ArrayBuffer(8); const r: any = ab.maxByteLength; r[k] = o;"
);
case!(
    array_buffer_resizable_is_a_boolean,
    false,
    "const ab: any = new ArrayBuffer(8); const r: any = ab.resizable; r[k] = o;"
);
case!(
    array_buffer_detached_is_a_boolean,
    false,
    "const ab: any = new ArrayBuffer(8); const r: any = ab.detached; r[k] = o;"
);
case!(
    shared_array_buffer_growable_is_a_boolean,
    false,
    "const sab: any = new SharedArrayBuffer(8); const r: any = sab.growable; r[k] = o;"
);
case!(
    data_view_buffer_is_the_viewed_buffer,
    false,
    "const dv: any = new DataView(new ArrayBuffer(8)); const r: any = dv.buffer; r[k] = o;"
);
case!(
    data_view_byte_length_is_a_number,
    false,
    "const dv: any = new DataView(new ArrayBuffer(8)); const r: any = dv.byteLength; r[k] = o;"
);
// Controls that keep their answer: an unknown receiver stays unknown; a
// value unknown code stored onto `ArrayBuffer.prototype` is read under an
// unknown key; a property the program stored on the buffer is read back.
case!(
    control_host_receiver_stays_unknown,
    true,
    "const ab: any = (globalThis as any).hostGive(); const r: any = ab[k]; r(o);"
);
case!(
    control_prototype_store_is_read,
    true,
    "(ArrayBuffer.prototype as any).zz = (globalThis as any).hostGive(); const ab: any = new ArrayBuffer(8); const r: any = ab[k]; r(o);"
);
case!(
    control_expando_is_read,
    true,
    "const ab: any = new ArrayBuffer(8); ab.foo = o; const r: any = ab[k]; r.leak();"
);
// `@@toStringTag` of each buffer kind (25.1.6.7, 25.2.5.7, 25.3.4.25): a
// String, under its symbol and under an unknown key.
case!(
    buffer_kinds_to_string_tag_is_a_string,
    false,
    "const ab: any = new ArrayBuffer(8); const sab: any = new SharedArrayBuffer(8); const dv: any = new DataView(ab); const a: any = ab[Symbol.toStringTag]; a[k] = o; const b: any = sab[Symbol.toStringTag]; b[k] = o; const c: any = dv[Symbol.toStringTag]; c[k] = o;"
);
case!(
    shared_array_buffer_unknown_key_read,
    false,
    "const sab: any = new SharedArrayBuffer(8); const r: any = sab[k]; r[k2] = o;"
);
// Control: a read on an unknown value holds ⊤.
top_case!(
    control_host_value_read_holds_top,
    true,
    "let x: any = (globalThis as any).hostGive(); x = x[k];"
);
