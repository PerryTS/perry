//! Typed array methods and constructors, `Date.prototype.toJSON` and
//! `new TextDecoder(label)` run user code through the conversions their
//! algorithms perform: ToNumber / ToIntegerOrInfinity of arguments, ToNumber
//! of a `map` callback or `sort` comparator result, the species constructor,
//! the `join` that `toString` looks up, array-like elements of a constructor
//! source, the `toISOString` that `toJSON` invokes, and the label's ToString.
//! `escapes`: node runs a method of `o`, so the facts must not prove it.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; leak() {{ (globalThis as any).hostTake(this); }} }}
        class ValueOf {{ size = 3; valueOf(): any {{ (globalThis as any).hostTake(this); return 1; }} }}
        class ToPrim {{ size = 3; [Symbol.toPrimitive](): any {{ (globalThis as any).hostTake(this); return 1; }} }}
        class ToStr {{ size = 3; toString(): any {{ (globalThis as any).hostTake(this); return 'utf-8'; }} }}
        function g(a: any) {{ return a.size; }}
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

case!(fill_runs_value_of, true,
 "const ta = new Uint8Array(4); const o: any = new ValueOf(); ta.fill(o); console.log(g(o));");
case!(slice_runs_to_primitive, true,
 "const ta = new Uint8Array(4); const o: any = new ToPrim(); ta.slice(o); console.log(g(o));");
case!(subarray_runs_value_of, true,
 "const ta = new Uint8Array(4); const o: any = new ValueOf(); ta.subarray(o); console.log(g(o));");
case!(map_result_is_coerced, true,
 "const ta = new Uint8Array(4); const o: any = new ValueOf(); ta.map(() => o); console.log(g(o));");
case!(sort_comparator_result_is_coerced, true,
 "const ta = new Uint8Array(4); const o: any = new ValueOf(); ta.sort(() => o); console.log(g(o));");
case!(map_callback_is_given_the_array, true,
 "class TA extends Uint8Array { size = 3; } const o: any = new TA(2); o.map((_v: any, _i: any, arr: any) => { (globalThis as any).hostTake(arr); return 0; }); console.log(g(o));");
case!(map_callback_gets_this_arg, true,
 "const ta = new Uint8Array(4); const o: any = new Svc(); ta.map(function (this: any) { this.leak(); return 0; }, o); console.log(g(o));");
case!(species_constructor_runs, true,
 "const ta: any = new Uint8Array(4); const o: any = new Svc(); ta.constructor = { [Symbol.species]: class C { constructor(n: number) { o.leak(); return new Uint8Array(n); } } }; ta.slice(0); console.log(g(o));");
case!(to_string_calls_a_replaced_join, true,
 "const ta = new Uint8Array(4); const o: any = new Svc(); (ta as any).join = function () { (globalThis as any).hostTake(o); return ''; }; ta.toString(); console.log(g(o));");
case!(constructor_array_like_elements_are_coerced, true,
 "const o: any = new ValueOf(); new Uint8Array({ length: 1, 0: o } as any); console.log(g(o));");
case!(date_to_json_calls_a_replaced_to_iso_string, true,
 "const d: any = new Date(0); const o: any = new Svc(); d.toISOString = () => { o.leak(); return ''; }; d.toJSON(); console.log(g(o));");
case!(text_decoder_label_is_stringified, true,
 "const o: any = new ToStr(); new TextDecoder(o); console.log(g(o));");

// The controls: the builtin conversions run nothing user-written.
case!(filter_result_is_only_tested, false,
 "const ta = new Uint8Array(4); const o: any = new ValueOf(); ta.filter(() => o); console.log(g(o));");
case!(builtin_methods_are_proven, false,
 "const ta = new Uint8Array(4); const o: any = new Svc(); ta.fill(1); ta.slice(1); ta.map((x) => x + 1); ta.sort((a, b) => a - b); ta.toString(); new Date(0).toJSON(); new TextDecoder('utf-8'); new Uint8Array([1, 2]); console.log(g(o));");
