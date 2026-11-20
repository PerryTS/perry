//! A member read on a primitive is found on the chain of its wrapper's
//! prototype (ECMA-262 7.3.2 GetV: ToObject, then 10.1.8.1 OrdinaryGet).
//! A kind whose chain has no member of that name reads `undefined` there,
//! never an unnamed builtin function: `repeat` on a Number, `toFixed` on a
//! Boolean, `foo` on any kind; calling what was read throws a TypeError
//! before `r` is used (7.3.14 Call). Controls: a kind that has the member
//! still reads its named native (`slice` on a String: its result is a
//! String); a value the program stored on a prototype of the chain
//! (`Number.prototype.repeat`, `Object.prototype.foo`) is read.
use super::*;

const MIXED: &str = "(process.argv.length > 9 ? 5 : \"s\")";

const READS: [(&str, &str); 6] = [
    (MIXED, "w.repeat"),
    ("(process.argv.length > 9 ? 5 : true)", "w.toFixed"),
    ("(process.argv.length > 9 ? \"s\" : true)", "w.slice"),
    ("5", "w.foo"),
    ("\"s\"", "w.nope"),
    ("true", "w.x"),
];

/// `r` is passed to what `{read}` reads on `w = {prim}`, after `{give}`.
fn read(prim: &str, read: &str, give: &str) -> crate::ProgramFacts {
    facts(&format!(
        r#"
        class R {{ y = 1; }}
        const o: any = {{ m(a: any) {{ return a.y; }} }};
        const r: any = new R();
        const w: any = {prim};
        {give}
        const f: any = {read};
        try {{ f(r); }} catch (e) {{}}
        try {{ {read}(r); }} catch (e) {{}}
        console.log(o.m(r));
    "#
    ))
}

fn minted(prim: &str, read: &str) -> u32 {
    let src = format!("const w: any = {prim}; const f: any = {read}; console.log(f);");
    super::box_computed_read::mints(&src, "primitive member")
}

fn escape(pf: &crate::ProgramFacts) -> Option<&str> {
    tmpl(pf, "R").escape.as_deref()
}

#[test]
fn a_member_no_wrapper_chain_of_the_kind_has_is_never_an_unnamed_function() {
    for (w, r) in READS {
        assert_eq!(minted(w, r), 0, "{w} {r}");
        let pf = read(w, r, "");
        assert_eq!(escape(&pf), None, "{w} {r}\n{}", pf.dump_text());
    }
}

/// Control: the kind that has the member reads its named native: the
/// result of `slice` on a String is a String, its `length` is proven.
#[test]
fn a_kind_with_the_member_reads_its_native() {
    for w in [MIXED, "\"s\""] {
        native_slice(w);
    }
}

fn native_slice(w: &str) {
    let src = format!(
        "const w: any = {w};\nconst f: any = w.slice;\n\
         const s: any = f.call(\"abc\", 1);\nconsole.log(s.length); // SITE-L\n"
    );
    let pf = facts(&src);
    let at = src.find("// SITE-L").unwrap() as u32;
    let start = src[..at as usize].rfind('\n').map_or(0, |i| i + 1) as u32;
    let sites: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == "length" && (start..at).contains(&s.byte_offset))
        .collect();
    assert!(!sites.is_empty(), "{}", pf.dump_text());
    assert!(
        sites.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        "{:?}\n{}",
        sites.iter().map(|s| &s.verdict).collect::<Vec<_>>(),
        pf.dump_text()
    );
}

/// Control: a value of the host the program stored on a prototype of the
/// chain under the name is read, and `r` reaches it.
#[test]
fn a_value_stored_on_the_chain_is_read() {
    for (w, r, k) in [
        (MIXED, "w.repeat", "(Number.prototype as any).repeat"),
        ("5", "w.foo", "(Object.prototype as any).foo"),
    ] {
        let pf = read(w, r, &format!("{k} = (globalThis as any).hostFn();"));
        assert!(escape(&pf).is_some(), "{w} {r}\n{}", pf.dump_text());
    }
}
