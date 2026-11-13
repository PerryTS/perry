//! No object of a typed array's chain (its kind's prototype,
//! `%TypedArray%.prototype`, `Object.prototype`) holds `@@toPrimitive`
//! (23.2.3: `%TypedArray%.prototype` holds `@@iterator` and
//! `@@toStringTag` only): its [[Get]] reads `undefined` (10.1.8.1), so
//! ToPrimitive (7.1.1) of a typed array, or of an object whose
//! [[Prototype]] is one, goes on to `valueOf`, which the chain finds on
//! `Object.prototype` (the realm lists the chain exactly), and `toString`,
//! named builtins: the object reaches no unknown function. Named controls: once the
//! program writes an unknown value as `@@toPrimitive` onto the chain, the
//! read finds it, as it does after a store under an unknown key; the
//! symbols the chain does hold (`@@iterator`) keep their members.
use super::*;

const KINDS: [&str; 3] = ["new Float64Array(2)", "new Uint8Array(2)", "new Int32Array(2)"];

/// `R` reaches no unknown code: no escape reason.
fn kept(pf: &crate::ProgramFacts) -> bool {
    tmpl(pf, "R").escape.is_none()
}

/// No call of an unknown function value has `R` as its receiver. (`R` may
/// still reach `%TypedArray%.prototype.join` through `toString`, a named
/// builtin; what that call does with a receiver that is no typed array is
/// another rule's.)
fn no_unknown_call(pf: &crate::ProgramFacts) -> bool {
    tmpl(pf, "R").escape.as_deref() != Some("receiver of a call of an unknown function value")
}

/// `r`'s [[Prototype]] is a typed array, given by `{give}`.
fn converted(prelude: &str, array: &str, give: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        {prelude}
        class R {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        const r: any = new R();
        const t: any = {array};
        {give}
        try {{ console.log("" + r); }} catch (e) {{}}
        console.log(o.m(r));
    "#
    ))
}

const SET: &str = "Object.setPrototypeOf(r, t);";
const STORE: &str = "r.__proto__ = t;";

#[test]
fn an_object_on_a_typed_array_has_no_to_primitive() {
    for array in KINDS {
        for give in [SET, STORE] {
            let pf = converted("", array, give);
            assert!(no_unknown_call(&pf), "{array} {give}\n{}", pf.dump_text());
        }
    }
}

/// A typed array itself as the `+` operand, holding an `R` (guard: its
/// own chain read is exact on the base already).
#[test]
fn a_typed_array_operand_has_no_to_primitive() {
    for array in KINDS {
        let pf = facts(&format!(
            r#"
            class R {{ y = 1; }}
            const o: any = {{ m(a: any) {{ return a.y; }} }};
            const t: any = {array};
            t.y = new R();
            try {{ console.log("" + t); }} catch (e) {{}}
            console.log(o.m(t));
        "#
        ));
        assert!(kept(&pf), "{array}\n{}", pf.dump_text());
    }
}

#[test]
fn a_written_to_primitive_is_read() {
    let pf = converted(
        "(Float64Array.prototype as any)[Symbol.toPrimitive] = (globalThis as any)[String(Math.random())];",
        "new Float64Array(2)",
        SET,
    );
    assert!(!kept(&pf), "{}", pf.dump_text());
}

/// A store under a key that is an unknown value may write `@@toPrimitive`
/// onto the chain: the read finds what was stored, as before.
#[test]
fn an_unknown_key_store_is_read() {
    let pf = converted(
        "const k: any = (globalThis as any).hostKey(); (Float64Array.prototype as any)[k] = (globalThis as any).hostFn();",
        "new Float64Array(2)",
        SET,
    );
    assert!(!kept(&pf), "{}", pf.dump_text());
}

/// `@@iterator` is on `%TypedArray%.prototype`: for-of over a typed array
/// runs the native iterator and binds numbers, never unknown values.
#[test]
fn a_typed_array_keeps_its_iterator() {
    for array in KINDS {
        let pf = facts(&format!(
            r#"
            class R {{ y = 1; }}
            const o: any = {{ m(a: any) {{ return a.y; }} }};
            for (const v of {array} as any) {{ console.log(o.m(v)); }}
            console.log(o.m(new R()));
            console.log(Object.prototype.toString.call({array}));
        "#
        ));
        assert!(!site(&pf, "y").top, "{array}\n{}", pf.dump_text());
    }
}
