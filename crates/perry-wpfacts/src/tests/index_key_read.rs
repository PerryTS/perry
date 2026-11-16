//! ECMA-262 7.1.19 ToPropertyKey, 10.1.8.1 [[Get]]: an index read is a
//! [[Get]] whose key is a Number converted to a canonical numeric String.
//! On an ordinary object that is not array-like it names the object's
//! number-named own properties (and what a computed store may have written
//! there), and along the program prototype chain only number-named members:
//! never a class method such as `m`. A native's element read of such an
//! object (`Array.from`, CreateListFromArrayLike of `apply` and
//! `Reflect.apply`, `new Float32Array(o)`) is that read. `escapes`: the
//! host may run `Holder::m`, which hands `svc` out, so the facts must not
//! prove `svc.size`.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        const svc: any = new Svc();
        class Holder {{ tag = "h"; m() {{ (globalThis as any).hostTake(svc); }} n() {{ return 1; }} }}
        function g(a: any) {{ return a.size; }}
        const take = (x: any) => (globalThis as any).hostTake(x);
        const i: any = Math.floor(Math.random() * 2);
        const h: any = new Holder();
        {body}
        console.log(g(svc));
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

macro_rules! case {
    ($name:ident, $escapes:expr, $body:expr) => {
        #[test]
        fn $name() {
            let v = verdict($body);
            assert_eq!(matches!(v, Verdict::No(_)), $escapes, "{v:?}");
        }
    };
}

// A Number key reads no method of the class prototype.
case!(program_index_reads_no_method, false, "take(h[i]);");
case!(native_from_reads_no_method, false, "take(Array.from(h)[0]);");
case!(native_apply_reads_no_method, false, "take.apply(undefined, h);");
case!(native_reflect_apply_reads_no_method, false, "Reflect.apply(take, undefined, h);");
case!(control_apply_index_field, true, "h[0] = svc; take.apply(undefined, h);");
case!(typed_array_source_reads_no_method, false, "take(new Float32Array(h));");

// Controls: the method itself escapes; a String key of unknown content
// still reads every member; a number-named own field is read.
case!(control_method_escapes, true, "take(h.m);");
case!(control_unknown_string_reads_method, true, "const s: any = (globalThis as any).hostKey(); take(h[s]);");
case!(control_native_unknown_string_reads_method, true, "const s: any = (globalThis as any).hostKey(); Array.prototype.forEach.call([h], (v: any) => take(v[s]));");
case!(control_index_field_read, true, "h[0] = svc; Array.prototype.forEach.call(h, (v: any) => take(v));");
case!(control_index_field_from, true, "h[0] = svc; take(Array.from(h)[0]);");
case!(control_program_index_field_read, true, "h[0] = svc; take(h[i]);");
