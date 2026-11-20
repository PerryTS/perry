//! A member the description does not name, read on a builtin object whose
//! [[Prototype]] chain the runtime realm declares, is what that chain
//! holds (ECMA-262 10.1.8.1 OrdinaryGet): `ArrayBuffer.prototype
//! [@@toStringTag]` is an own String (25.1.6.7); `RegExp.prototype` and
//! the NativeError prototypes own no `@@toStringTag` (22.2.6, 20.5.6.3):
//! `undefined`; `%TypedArray%.prototype[@@toStringTag]` is a getter that
//! returns a String or `undefined` (23.2.3.38); `BYTES_PER_ELEMENT` is a
//! Number (23.2.7.2); `Blob.prototype` and `Response.prototype` own no
//! `then`: `undefined`. Never a builtin function the realm does not name;
//! calling what was read throws a TypeError before `r` is used (7.3.14).
//! Controls: a value of the host stored onto the chain under that key is
//! read.
use super::*;

const READS: [(&str, &str); 6] = [
    ("new ArrayBuffer(8)", "w[Symbol.toStringTag]"),
    ("/a/", "w[Symbol.toStringTag]"),
    ("new Uint8Array(2)", "w[Symbol.toStringTag]"),
    ("new Uint8Array(2)", "w.BYTES_PER_ELEMENT"),
    ("Blob.prototype", "w.then"),
    ("Response.prototype", "w.then"),
];

/// Reads whose value is the realm's but whose call still escapes `r` for
/// another reason: on `Blob.prototype` and `Response.prototype` the
/// unknown-function value of the call is the ⊤ of a `then` the solve
/// takes as possibly written onto `%Object.prototype%`, not a builtin
/// function. (`TypeError.prototype` is not read here: its value is the
/// token of every builtin prototype, whose `@@toStringTag` some prototype
/// owns.)
const ESCAPED_ELSEWHERE: [&str; 2] = ["Blob.prototype", "Response.prototype"];

/// `r` is passed to what `{read}` reads on `w = {wrapper}`, after `{give}`.
fn read(wrapper: &str, read: &str, give: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        class R {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        const r: any = new R();
        const w: any = {wrapper};
        const P: any = Object.prototype;
        {give}
        const f: any = {read};
        try {{ f(r); }} catch (e) {{}}
        console.log(o.m(r));
    "#
    ))
}

fn minted(wrapper: &str, read: &str) -> u32 {
    let src = format!("const w: any = {wrapper}; const f: any = {read}; console.log(f);");
    super::box_computed_read::mints(&src, "undescribed member")
}

fn escape(pf: &crate::ProgramFacts) -> Option<&str> {
    tmpl(pf, "R").escape.as_deref()
}

#[test]
fn a_member_the_realm_declares_on_the_chain_is_never_an_unnamed_function() {
    let mut bad = Vec::new();
    for (w, r) in READS {
        let n = minted(w, r);
        let pf = read(w, r, "");
        if n != 0 || (escape(&pf).is_some() && !ESCAPED_ELSEWHERE.contains(&w)) {
            bad.push(format!("{w} {r}: mints {n}, escape {:?}", escape(&pf)));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Control: a value of the host stored onto `Object.prototype` under the
/// key, which the realm leaves absent on the chain, is read.
#[test]
fn a_value_stored_on_the_chain_is_read() {
    for (w, r, k) in [
        ("/a/", "w[Symbol.toStringTag]", "P[Symbol.toStringTag]"),
        ("Blob.prototype", "w.then", "P.then"),
    ] {
        let pf = read(w, r, &format!("{k} = (globalThis as any).hostFn();"));
        assert!(escape(&pf).is_some(), "{w} {r}\n{}", pf.dump_text());
    }
}
