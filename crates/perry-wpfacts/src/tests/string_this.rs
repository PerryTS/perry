//! A `String.prototype` method called through `.call` with a receiver that
//! is no string (tsc's `stringReplace.call(s, "*", r)`, utilities.ts): the
//! method converts its receiver with `ToString` (ES 22.1.3) and runs nothing
//! else on it, so the receiver is not handed to unknown code. A callee that
//! may also be unknown code still hands it over.
use super::*;

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
    let s = at(pf, src, marker, key);
    format!(
        "{:?}",
        s.iter()
            .map(|s| (&s.receivers, &s.verdict))
            .collect::<Vec<_>>()
    )
}

fn proven(pf: &ProgramFacts, src: &str, marker: &str, key: &str) -> bool {
    (at(pf, src, marker, key).iter()).all(|s| !matches!(s.verdict, Verdict::No(_)))
}

/// `p` reaches the receiver of the extracted `replace`; its field is
/// read after the call.
fn program(callee: &str) -> String {
    format!(
        "class P {{ x: number; constructor() {{ this.x = 1; }} }}\n\
         const stringReplace: any = {callee};\n\
         function star(s: any, r: string): string {{ return stringReplace.call(s, \"*\", r); }}\n\
         const p = new P();\n\
         const q: any = process.argv.length > 9 ? p : \"a*b\";\n\
         console.log(star(q, \"c\"));\n\
         console.log(p.x); // SITE\n"
    )
}

/// Red on main: the call of the named native with a receiver outside its
/// family was an unknown callee's call, which escaped `p`.
#[test]
fn a_string_method_on_an_object_receiver_hands_it_to_no_one() {
    let src = program("String.prototype.replace");
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// Control: the callee may also be a function unseen code stored on the
/// global object; that one may keep `p` and change it.
#[test]
fn a_maybe_unknown_callee_still_hands_the_receiver_over() {
    let src = unseen(&program(
        "process.argv.length > 8 ? String.prototype.replace : (globalThis as any).hostReplace",
    ));
    let pf = facts(&src);
    assert!(
        !proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// The conversion runs the receiver's own `toString` with `this` = the
/// receiver: what it stores is seen (here an `A` in `v.s`).
#[test]
fn the_receiver_conversion_runs_its_own_to_string() {
    let src = "class A { a = 1; }\n\
        class V { s: any; constructor() { this.s = undefined; } toString() { this.s = new A(); return \"v*\"; } }\n\
        const stringReplace = String.prototype.replace;\n\
        const v = new V();\n\
        console.log(stringReplace.call(v as any, \"*\", \"c\"));\n\
        console.log(v.s.a); // SITE\n";
    let pf = facts(src);
    let v = verdicts(&pf, src, "// SITE", "a");
    assert!(v.contains('A'), "{v}");
    assert!(proven(&pf, src, "// SITE", "a"), "{v}");
}

/// Control: the conversion is a real call of the receiver's `toString`
/// with `this` = the receiver; one that hands `this` to unseen code
/// (a global unseen code may read) keeps the receiver open. The native is
/// taken from a string, not from `String`: unseen code may rewrite that global.
#[test]
fn a_to_string_that_leaks_this_keeps_the_receiver_open() {
    let src = unseen(
        "class P { x: number; constructor() { this.x = 1; } toString() { (globalThis as any).leaked = this; return \"p*\"; } }\n\
         const stringReplace = \"\".replace;\n\
         const p = new P();\n\
         console.log(stringReplace.call(p as any, \"*\", \"c\"));\n\
         console.log(p.x); // SITE\n",
    );
    let pf = facts(&src);
    assert!(
        !proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// Twin of the control without the leak: proven (so the control fails
/// for the leak, not for the program shape).
#[test]
fn a_to_string_that_keeps_this_leaves_the_receiver_proven() {
    let src = unseen(
        "class P { x: number; constructor() { this.x = 1; } toString() { return \"p*\"; } }\n\
         const stringReplace = \"\".replace;\n\
         const p = new P();\n\
         console.log(stringReplace.call(p as any, \"*\", \"c\"));\n\
         console.log(p.x); // SITE\n",
    );
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}
