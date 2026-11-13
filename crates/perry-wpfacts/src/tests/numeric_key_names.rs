//! ECMA-262 7.1.19 ToPropertyKey: a Number key is Number::toString
//! (6.1.6.1.20: digits, "-", ".", an exponent, "NaN", "Infinity"), a
//! BigInt key BigInt::toString (6.1.6.2.23). On an Array (10.4.2.1: an
//! array index names an element, any other name an ordinary property) and
//! on an ordinary object such a store names one of those names: never
//! `toJSON`, `foo` or another non-numeric name. Every numeric name, an
//! index, an unknown String key and an enumeration may read it.
//! `escapes`: node runs a method of `o`, so the facts must not prove it.

use super::*;

fn verdict(recv: &str, body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.size; }}
        const k = Math.floor(Math.random() * 2);
        const w: any = k > 0 ? 3n : 2;
        const take = (x: any) => (globalThis as any).hostTake(x);
        const r: any = {recv};
        const o: any = new Svc();
        {body}
        console.log(g(o));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

macro_rules! case {
    ($name:ident, $escapes:expr, $recv:expr, $body:expr) => {
        #[test]
        fn $name() {
            let v = verdict($recv, $body);
            assert_eq!(matches!(v, Verdict::No(_)), $escapes, "{v:?}");
        }
    };
}

const ARR: &str = "[] as any[]";
const OBJ: &str = "{}";

// (a) An Array: a Number-or-BigInt key never names `toJSON` or `foo`.
case!(arr_digits_names_no_to_json, false, ARR, "r[w] = o; take(r.toJSON);");
case!(arr_digits_names_no_foo, false, ARR, "r[w] = o; take(r.foo); take(r.valueOf); take(r.toString);");
// (b) A plain object: the same.
case!(obj_digits_names_no_to_json, false, OBJ, "r[w] = o; take(r.toJSON);");
case!(obj_digits_names_no_foo, false, OBJ, "r[w] = o; take(r.foo); take(r.valueOf); take(r.toString);");

// (c) Every index, index string, unknown String key and enumeration may read it.
case!(arr_digits_index_2, true, ARR, "r[w] = o; take(r[2]);");
case!(arr_digits_index_3, true, ARR, "r[w] = o; take(r[3]);");
case!(arr_digits_index_string, true, ARR, "r[w] = o; take(r[\"3\"]);");
case!(arr_digits_unknown_string, true, ARR, "r[w] = o; const s: any = (globalThis as any).hostKey(); take(r[s]);");
case!(arr_digits_for_in, true, ARR, "r[w] = o; for (const p in r) take(r[p]);");
case!(arr_digits_values, true, ARR, "r[w] = o; take(Object.values(r));");
case!(arr_bigint_only_index, true, ARR, "const b: any = k > 0 ? 3n : 4n; r[b] = o; take(r[3]);");
case!(arr_bigint_only_names_no_to_json, false, ARR, "const b: any = k > 0 ? 3n : 4n; r[b] = o; take(r.toJSON);");
case!(obj_digits_index_3, true, OBJ, "r[w] = o; take(r[3]);");
case!(obj_digits_index_string, true, OBJ, "r[w] = o; take(r[\"3\"]);");
case!(obj_digits_long_digits, true, OBJ, "r[w] = o; take(r[\"12345678901234567890\"]);");
case!(obj_digits_unknown_string, true, OBJ, "r[w] = o; const s: any = (globalThis as any).hostKey(); take(r[s]);");
case!(obj_digits_for_in, true, OBJ, "r[w] = o; for (const p in r) take(r[p]);");
case!(obj_digits_values, true, OBJ, "r[w] = o; take(Object.values(r));");
// (c) A pure Number key (the element store into `#num`): for-in lists its name.
case!(obj_num_for_in, true, OBJ, "r[k] = o; for (const p in r) take(r[p]);");
case!(obj_num_fraction_for_in, true, OBJ, "const n: any = k > 0 ? 1.5 : NaN; r[n] = o; for (const p in r) take(r[p]);");
case!(arr_num_for_in, true, ARR, "r[k] = o; for (const p in r) take(r[p]);");
case!(obj_bigint_only_index, true, OBJ, "const b: any = k > 0 ? 3n : 4n; r[b] = o; take(r[\"3\"]);");

// (d) Number::toString forms that are no digits: NaN, Infinity, "-1", "1.5", "1e+21".
case!(arr_num_nan, true, ARR, "const n: any = k > 0 ? NaN : 1e21; r[n] = o; take(r.NaN);");
case!(arr_num_exp, true, ARR, "const n: any = k > 0 ? NaN : 1e21; r[n] = o; take(r[\"1e+21\"]);");
case!(arr_num_infinity, true, ARR, "const n: any = k > 0 ? Infinity : -1; r[n] = o; take(r.Infinity);");
case!(arr_num_minus_one, true, ARR, "const n: any = k > 0 ? Infinity : -1; r[n] = o; take(r[\"-1\"]);");
case!(arr_num_fraction, true, ARR, "const n: any = k > 0 ? 1.5 : 2; r[n] = o; take(r[\"1.5\"]);");
case!(obj_num_nan, true, OBJ, "const n: any = k > 0 ? NaN : 1e21; r[n] = o; take(r.NaN);");
case!(obj_num_exp, true, OBJ, "const n: any = k > 0 ? NaN : 1e21; r[n] = o; take(r[\"1e+21\"]);");
case!(obj_num_infinity, true, OBJ, "const n: any = k > 0 ? Infinity : -1; r[n] = o; take(r.Infinity);");
case!(obj_num_minus_one, true, OBJ, "const n: any = k > 0 ? Infinity : -1; r[n] = o; take(r[\"-1\"]);");
case!(obj_num_fraction, true, OBJ, "const n: any = k > 0 ? 1.5 : 2; r[n] = o; take(r[\"1.5\"]);");
case!(obj_digits_minus_infinity, true, OBJ, "const n: any = k > 0 ? -Infinity : 2; r[n] = o; take(r[\"-Infinity\"]);");

// (e) Control: a key that may be a String may name it.
case!(arr_string_key_names_x, true, ARR, "const x: any = k > 0 ? \"x\" : 2; r[x] = o; take(r.x);");
case!(obj_string_key_names_x, true, OBJ, "const x: any = k > 0 ? \"x\" : 2; r[x] = o; take(r.x);");
// (f) Control: an unknown key may name `toJSON`.
case!(arr_unknown_key_names_to_json, true, ARR, "const s: any = (globalThis as any).hostKey(); r[s] = o; take(r.toJSON);");
case!(obj_unknown_key_names_to_json, true, OBJ, "const s: any = (globalThis as any).hostKey(); r[s] = o; take(r.toJSON);");
