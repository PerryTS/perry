//! A primitive wrapper's chain is the realm's: `Object(value)` (20.1.1.1)
//! makes an ordinary object or, by ToObject (7.1.18), a wrapper of the
//! value's kind, whose [[Prototype]] is `String`/`Number`/`Boolean`/
//! `Symbol`/`BigInt.prototype`. The runtime realm declares those
//! prototypes' own keys exactly: none of them, nor `Object.prototype`,
//! holds `@@toPrimitive` (the runtime's `Symbol.prototype` holds
//! `constructor`, `toString`, `valueOf` only), so its [[Get]] reads
//! `undefined` (10.1.8.1) and ToPrimitive (7.1.1) of an object whose
//! [[Prototype]] is such a wrapper goes on to `valueOf`/`toString`, the
//! named natives of the prototypes the wrapper may have: the object reaches
//! no unknown function. Controls: an unknown value written as
//! `@@toPrimitive` onto the chain is read; a `Date` and a Symbol wrapper
//! operand keep their behaviour.
use super::*;

const WRAPPERS: [&str; 4] = [
    "Object(Symbol(\"s\"))",
    "Object(1)",
    "new Object(\"a\")",
    "Object()",
];

/// No call of an unknown function value has `R` as its receiver.
fn no_unknown_call(pf: &crate::ProgramFacts) -> bool {
    tmpl(pf, "R").escape.as_deref() != Some("receiver of a call of an unknown function value")
}

/// `r`'s [[Prototype]] is a wrapper, given by `{give}`.
fn converted(prelude: &str, wrapper: &str, give: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        {prelude}
        class R {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        const r: any = new R();
        const t: any = {wrapper};
        {give}
        try {{ console.log("" + r); }} catch (e) {{}}
        console.log(o.m(r));
    "#
    ))
}

const SET: &str = "Object.setPrototypeOf(r, t);";
const STORE: &str = "r.__proto__ = t;";

#[test]
fn an_object_on_a_wrapper_has_no_to_primitive() {
    for wrapper in WRAPPERS {
        for give in [SET, STORE] {
            let pf = converted("", wrapper, give);
            assert!(no_unknown_call(&pf), "{wrapper} {give}\n{}", pf.dump_text());
        }
    }
}

/// Control: a function value the program does not know, written under
/// `Symbol.toPrimitive` onto `Object.prototype` (on every chain), is read.
#[test]
fn a_written_to_primitive_is_read() {
    let pf = converted(
        "(Object.prototype as any)[Symbol.toPrimitive] = (globalThis as any).hostFn();",
        "Object(Symbol(\"s\"))",
        SET,
    );
    assert!(tmpl(&pf, "R").escape.is_some(), "{}", pf.dump_text());
}

/// Guards: a `Date` operand (whose prototype does hold `@@toPrimitive`)
/// and a Symbol wrapper operand, each carrying an `R`.
#[test]
fn date_and_symbol_wrapper_operands() {
    for operand in ["new Date(0)", "Object(Symbol(\"s\"))"] {
        let pf = facts(&format!(
            r#"
            class R {{ y = 1; }}
            const o: any = {{ m(a: any) {{ return a.y; }} }};
            const t: any = {operand};
            t.y = new R();
            try {{ console.log("" + t); }} catch (e) {{}}
            console.log(o.m(t));
        "#
        ));
        assert!(no_unknown_call(&pf), "{operand}\n{}", pf.dump_text());
    }
}
