//! A primitive family's native called through `.call` with a primitive
//! `this` of another family (tsc's `stringReplace.call(s, "*", r)` with a
//! number `s`): a `String.prototype` method runs `ToString(this)` first,
//! which on a number, boolean or bigint is the realm's own conversion, so the
//! method runs on a string (on a symbol it throws); a `Number`, `Boolean`,
//! `Symbol` or `BigInt` method runs `thisNumberValue(this)` (and the like)
//! first, which throws. Neither hands anything to unknown code.
use super::*;
use crate::model::prim;

fn at<'a>(pf: &'a ProgramFacts, src: &str, marker: &str, key: &str) -> Vec<&'a SiteFacts> {
    let m = src
        .find(marker)
        .unwrap_or_else(|| panic!("marker {marker}"));
    let start = src[..m].rfind('\n').map_or(0, |i| i + 1) as u32;
    let s: Vec<_> = (pf.sites.iter())
        .filter(|s| !s.is_call && s.key == key && (start..m as u32).contains(&s.byte_offset))
        .collect();
    assert!(!s.is_empty(), "no `{key}` site on the {marker} line");
    s
}

fn verdicts(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> String {
    format!(
        "{:?}",
        (at(pf, src, marker, key).iter())
            .map(|s| (&s.receivers, s.receiver_prims, s.top, &s.verdict))
            .collect::<Vec<_>>()
    )
}

fn proven(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> bool {
    (at(pf, src, marker, key).iter()).all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// Red on main: `replace` with a number `this` was an unknown callee's
/// call, which escaped its replacement argument `p`. The method converts
/// `p` with `ToString` and keeps nothing of it.
#[test]
fn a_string_method_on_a_number_this_runs_on_a_string() {
    let src = "class Payload { n = 1; }\n\
        const stringReplace = String.prototype.replace;\n\
        function rep(s: any, r: any): string { return stringReplace.call(s, \"*\", r); }\n\
        const p = new Payload();\n\
        const a = rep(42, p);\n\
        const b = rep(\"a*b\", \"y\");\n\
        const out = { s: a + b, p };\n\
        console.log(out.p.n); // SITE\n";
    let pf = facts(src);
    let v = verdicts(&pf, src, "// SITE", "n");
    assert!(proven(&pf, src, "// SITE", "n"), "{v}");
    assert!(
        (at(&pf, src, "// SITE", "n").iter())
            .all(|s| s.receivers.len() == 1 && s.receivers[0].contains("Payload")),
        "{v}"
    );
}

/// Red on main: each method starts with a check of its `this`
/// (`thisNumberValue`, `thisBooleanValue`, `thisSymbolValue`,
/// `thisBigIntValue`; `ToString` of a symbol for a `String` method), which
/// throws on the foreign primitive before any argument is read: `p`
/// reaches no one.
#[test]
fn a_method_on_a_foreign_primitive_this_throws_and_sinks_no_argument() {
    let cases = [
        ("Number.prototype.toString", "\"1\""),
        ("Boolean.prototype.valueOf", "1"),
        ("Symbol.prototype.toString", "\"1\""),
        ("BigInt.prototype.toString", "true"),
        ("String.prototype.replace", "Symbol()"),
    ];
    let mut open = Vec::new();
    for (f, this) in cases {
        let src = format!(
            "class P {{ x: number; constructor() {{ this.x = 1; }} }}\n\
             const g: any = {f};\n\
             function tf(s: any, a: any) {{ return g.call(s, a, a); }}\n\
             const p = new P();\n\
             try {{ console.log(tf({this}, p)); }} catch (e) {{}}\n\
             console.log(p.x); // SITE\n"
        );
        let pf = facts(&src);
        if !proven(&pf, &src, "// SITE", "x") {
            open.push(format!(
                "{f} on {this}: {}",
                verdicts(&pf, &src, "// SITE", "x")
            ));
        }
    }
    assert!(open.is_empty(), "{open:#?}");
}

/// Control twin of the throw case: the callee may also be a function
/// unseen code stored on the global object; that one may keep `p`.
#[test]
fn a_maybe_unknown_callee_on_a_primitive_this_still_hands_the_argument_over() {
    let src = unseen(
        "class P { x: number; constructor() { this.x = 1; } }\n\
         const g: any = process.argv.length > 8 ? Number.prototype.toString : (globalThis as any).hostFn;\n\
         function tf(s: any, a: any) { return g.call(s, a); }\n\
         const p = new P();\n\
         try { console.log(tf(\"1\", p)); } catch (e) {}\n\
         console.log(p.x); // SITE\n",
    );
    let pf = facts(&src);
    assert!(
        !proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// Named control: a `replace` callback with a number `this` is still
/// called, with a string match (on main: by unknown code, with ⊤). Running
/// the method as "no callee" leaves `m` with no value at all.
#[test]
fn a_replace_callback_on_a_number_this_is_reached_with_a_string() {
    let src = "const stringReplace = String.prototype.replace;\n\
        function rep(s: any, f: any): string { return stringReplace.call(s, \"4\", f); }\n\
        console.log(rep(42, (m: any) => { console.log(m.length); // CB\n return \"\"; }));\n";
    let pf = facts(src);
    let cb = verdicts(&pf, src, "// CB", "length");
    assert!(
        (at(&pf, src, "// CB", "length").iter())
            .all(|s| s.top || s.receiver_prims & prim::STR != 0),
        "{cb}"
    );
}
