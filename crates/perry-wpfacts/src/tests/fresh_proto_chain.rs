//! W2a: an object a realm declaration's `fresh(P)` makes has `P` as its
//! [[Prototype]]. A method call on it finds the realm-declared own property
//! of the first declaration on `P`'s declared chain; a key no declaration
//! on the chain has is absent (a TypeError when called), unless a store
//! into ⊤ may have written it. The F-tests are red without the rule (the
//! call ran as an unknown method and the program object escaped); the
//! K-test is the named control.
use super::*;

fn esc(pf: &ProgramFacts, class: &str) -> bool {
    tmpl(pf, class).escape.is_some()
}

/// F1 (skin M1, FileLoader::load): the then-callback returns fetch's
/// declared Response, so resolution reads its `then`, which neither
/// `Response.prototype` nor `Object.prototype` has (node: fulfilled with
/// the Response, nothing called). The next callback's `v.f(h)` is a call of
/// an absent member: a TypeError, `h` handed to no one.
#[test]
fn f1_a_resolved_response_is_no_thenable() {
    let pf = facts(
        r#"
        class H { a = 1; }
        const h = new H();
        fetch(process.argv[2]).then((r: any) => r).then((v: any) => v.f(h));
        console.log(h.a);
        export const keep = 1;
    "#,
    );
    assert!(!esc(&pf, "H"), "{}", pf.dump_text());
}

/// F2 (skin M2): an aborted signal's reason is a fresh DOMException. Its
/// chain (DOMException.prototype -> Error.prototype -> Object.prototype)
/// finds `toString` on Error.prototype and `valueOf` / `hasOwnProperty` on
/// Object.prototype, which run their declared effects: toString reads
/// `name`/`message`, valueOf returns the receiver, both ignore their
/// arguments; hasOwnProperty converts its key (K's own `toString` runs, K
/// stays the program's). (fetch's TypeError reaches the same members once
/// the runtime stops copying them onto every builtin prototype.)
#[test]
fn f2_a_dom_exception_runs_its_declared_chain_methods() {
    let pf = facts(
        r#"
        class H { a = 1; }
        class K { toString() { return "k"; } }
        const h = new H(); const k = new K();
        const e: any = AbortSignal.abort().reason;
        console.log([e.toString(h), e.valueOf(h), e.hasOwnProperty(k), String(e)]);
        console.log(h.a, String(k));
        export const keep = 1;
    "#,
    );
    assert!(!esc(&pf, "H"), "{}", pf.dump_text());
    assert!(!esc(&pf, "K"), "{}", pf.dump_text());
}

/// K1, the named control: a store under a computed key on
/// `Response.prototype` may add any key, `f` included, so `v.f(h)` stays a
/// call of what may have been stored there (an unknown method): `h`
/// escapes. A rule that took the realm's key set as final would prove it.
#[test]
fn k1_a_computed_write_on_the_prototype_keeps_the_call() {
    let pf = facts(
        r#"
        (Response.prototype as any)[process.argv[3]] = function (x: any) { return x; };
        class H { a = 1; }
        const h = new H();
        fetch(process.argv[2]).then((r: any) => r).then((v: any) => v.f(h));
        console.log(h.a);
        export const keep = 1;
    "#,
    );
    assert!(esc(&pf, "H"), "{}", pf.dump_text());
}
