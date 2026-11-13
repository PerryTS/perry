//! Every `%TypedArray%.prototype` method starts with ValidateTypedArray
//! (ECMA-262 23.2.4.4) or a [[TypedArrayName]] check: called with a `this`
//! that is no typed array it throws a TypeError before it uses the receiver
//! or any argument (the runtime: `typed_array_proto_thunks.rs`
//! `ta_receiver_or_throw`). A program object, a function or a builtin
//! function reached as `this` of such a method (by `.call`, or by
//! ToPrimitive's `toString` -> `join` on an object whose prototype is a
//! typed array) reaches no code. Controls: a real typed array receiver
//! keeps the modelled method, an unknown receiver keeps the unknown call.
use super::*;

fn check(body: &str, yes: bool) {
    let src = format!(
        r#"
        (globalThis as any).hostTake = (_x: any) => {{}};
        class Svc {{ marker = 3; length = 1; leak() {{ (globalThis as any).hostTake(this); }} }}
        function g(a: any) {{ return a.marker; }}
        const o: any = new Svc(); const a: any = new Uint8Array([1, 2]);
        const j: any = Uint8Array.prototype.join;
        {body}
        console.log(g(o));
    "#
    );
    let pf = facts(&src);
    let v = &site(&pf, "marker").verdict;
    assert_eq!(!matches!(v, Verdict::No(_)), yes, "{body}: {v:?}\n{}", pf.dump_text());
}

/// `this` no typed array: a program object, a function, a builtin function.
#[test]
fn a_typed_array_method_on_no_typed_array_throws() {
    for op in [
        "try { j.call(o, \",\"); } catch (e) {}",
        "try { j.call(Object.assign, o); } catch (e) {}",
        "try { j.call(function f() {}, o); } catch (e) {}",
        "try { j.call({}, o); } catch (e) {}",
        "try { (Float32Array.prototype as any).map.call({}, () => o); } catch (e) {}",
        "try { (Int16Array.prototype as any).set.call(Object.assign, o); } catch (e) {}",
    ] {
        check(op, true);
    }
}

/// ToPrimitive of an object whose [[Prototype]] is a typed array:
/// `toString` (%TypedArray%.prototype.toString = Array.prototype.toString)
/// invokes `join` on the object, which is no typed array. (Giving a class
/// instance a prototype hard-marks the class; the claim checked is `R`'s
/// escape.)
#[test]
fn to_primitive_on_a_typed_array_prototype_throws() {
    for give in ["Object.setPrototypeOf(r, t);", "r.__proto__ = t;"] {
        let pf = facts(&format!(
            r#"
            class R {{ y = 1; }}
            const o: any = {{ m(a: any) {{ return a.y; }} }};
            const r: any = new R();
            const t: any = new Uint8Array(2);
            {give}
            try {{ console.log("" + r); }} catch (e) {{}}
            console.log(o.m(r));
        "#
        ));
        assert_eq!(tmpl(&pf, "R").escape.as_deref().unwrap_or(""), "", "{give}\n{}", pf.dump_text());
    }
}

/// Control: a real typed array receiver runs the modelled `join`
/// (its separator only coerced).
#[test]
fn a_typed_array_receiver_keeps_the_method() {
    check("console.log(j.call(a, o));", true);
}

/// Control (the fixture can fail): an unknown receiver may be a typed
/// array whose method reaches unknown code; `o` handed to an unknown
/// function escapes.
#[test]
fn an_unknown_receiver_keeps_the_unknown_call() {
    check("try { j.call((globalThis as any).hostGet(), o); } catch (e) {}", false);
}
