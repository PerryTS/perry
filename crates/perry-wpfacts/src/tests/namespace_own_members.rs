//! A member read on a realm constructor or namespace object finds what that
//! object owns, else what its [[Prototype]] chain holds, else `undefined`
//! (ECMA-262 10.1.8.1 OrdinaryGet). The runtime realm installs on `Array`
//! only `isArray`, `from` and `of` (23.1.2; perry-runtime
//! install_static.rs); `Array.fromAsync` read as a value is no property of
//! it and reads `undefined`, never some builtin function the description
//! does not name. Control: a value of the host stored onto
//! `Function.prototype` (its chain) under that key is read.
use super::*;

/// `r` is passed to what `{read}` reads, after `{give}`.
fn read(read: &str, give: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        class R {{ y = 1; }}
        const A: any = Array;
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        const r: any = new R();
        {give}
        const f: any = {read};
        try {{ f(r); }} catch (e) {{}}
        console.log(o.m(r));
    "#
    ))
}

/// The unnamed builtin functions `ns_load` makes for `read`.
fn minted(read: &str) -> u32 {
    let src = format!(
        r#"
        const A: any = Array;
        const k: string = String((globalThis as any).hostKey());
        const f: any = {read};
        console.log(f);
    "#
    );
    super::box_computed_read::mints(&src, "builtin namespace static")
}

fn escape(pf: &crate::ProgramFacts) -> Option<&str> {
    tmpl(pf, "R").escape.as_deref()
}

/// A computed key reads the members `Array` owns and its chain's, named.
#[test]
fn a_computed_key_on_a_constructor_reads_what_it_owns() {
    assert_eq!(minted("A[k]"), 0);
}

#[test]
fn a_static_the_realm_does_not_install_reads_undefined() {
    assert_eq!(minted("A.fromAsync"), 0);
    let pf = read("A.fromAsync", "");
    assert_eq!(escape(&pf), None, "{}", pf.dump_text());
}

/// Control: a value of the host stored onto the constructor's chain.
#[test]
fn a_value_on_its_chain_is_read() {
    let pf = read(
        "A.fromAsync",
        "(Function.prototype as any).fromAsync = (globalThis as any).hostFn();",
    );
    assert!(escape(&pf).is_some(), "{}", pf.dump_text());
}
