//! A computed key on a primitive wrapper reads its realm chain: `Object(v)`
//! (20.1.1.1) is an ordinary object or, by ToObject (7.1.18), a wrapper of
//! `v`'s kind whose [[Prototype]] is `String`/`Number`/`Boolean`/`Symbol`/
//! `BigInt.prototype`. The runtime realm declares each of those prototypes
//! and `Object.prototype` exactly, so [[Get]] (10.1.8.1 OrdinaryGet) of a
//! key of unknown spelling finds the wrapper's own value, a member those
//! chains declare under a name the key may spell, or `undefined`: never a
//! builtin function the description does not name. The same holds for a
//! typed array (`%TypedArray%.prototype`) and an `ArrayBuffer`, whose
//! chains the realm declares too. Controls: a value the
//! program stored on the wrapper, or onto a prototype of its chain, under a
//! computed key, is read.
use super::*;

const WRAPPERS: [&str; 6] = [
    "Object(1)",
    "Object(\"a\")",
    "new Object(true)",
    "Object()",
    "new Uint8Array(2)",
    "new ArrayBuffer(8)",
];

/// `r` is passed to what `w[k]` reads (`k` a string the program does not
/// know), after `{give}`.
fn read(wrapper: &str, give: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        class R {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        const r: any = new R();
        const w: any = {wrapper};
        const k: string = String((globalThis as any).hostKey());
        {give}
        const f: any = w[k];
        try {{ f(r); }} catch (e) {{}}
        console.log(o.m(r));
    "#
    ))
}

fn opaque(pf: &crate::ProgramFacts) -> bool {
    tmpl(pf, "R").escape.as_deref() == Some("passed to an opaque builtin function value")
}

#[test]
fn a_computed_key_on_a_wrapper_reads_its_realm_chain() {
    for w in WRAPPERS {
        let pf = read(w, "");
        assert!(!opaque(&pf), "{w}\n{}", pf.dump_text());
    }
}

/// Control: a value of the host stored on the wrapper under a computed key.
#[test]
fn an_own_value_is_read() {
    let pf = read("Object(1)", "w[String((globalThis as any).hostKey())] = (globalThis as any).hostFn();");
    assert!(tmpl(&pf, "R").escape.is_some(), "{}", pf.dump_text());
}

/// Control: a value of the host stored onto `Number.prototype` under a
/// computed key.
#[test]
fn a_value_stored_on_the_chain_is_read() {
    let pf = read(
        "Object(1)",
        "(Number.prototype as any)[String((globalThis as any).hostKey())] = (globalThis as any).hostFn();",
    );
    assert!(tmpl(&pf, "R").escape.is_some(), "{}", pf.dump_text());
}
