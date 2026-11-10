//! A bare call of a global function (`fetch(u)`) is made on the
//! unresolved-global sentinel, the global namespace's own object, through
//! the member's native value. That native runs on the receiver token it is
//! called for, as a call by name on it does: the namespace's own object is
//! given to no one, so `Promise` and every other global read off it stay
//! what they are (three.js FileLoader's `fetch( req )`). A program object
//! the native is called on, and every argument, still reach unknown code.
//! A global function whose effects the runtime declares (`fetch`) runs its
//! declaration, which names no receiver: node's `fetch` never reads `this`.
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

/// `p` reaches a promise callback; the global function call `call` runs
/// in a branch the solve does not prune.
fn program(call: &str) -> String {
    format!(
        "class P {{ x: number; constructor() {{ this.x = 1; }} }}\n\
         const p = new P();\n\
         function load(u: string): any {{ {call} }}\n\
         if (process.argv.length > 9) load(\"http://x/\");\n\
         Promise.resolve(p).then((v: P) => console.log(v.x)); // SITE\n"
    )
}

/// Red on main: the call ran with no receiver token, so the model sank the
/// receiver node and with it the sentinel: `Promise` became unknown code.
#[test]
fn a_bare_global_call_gives_the_global_namespace_to_no_one() {
    let src = program("return fetch(u);");
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// Control: called as a method of a program object, a native whose effects
/// are not modeled (`structuredClone` read as a value) is given that object
/// as `this` (`o.f(u)`): it reaches unknown code, `p` with it.
#[test]
fn a_program_receiver_of_the_native_still_escapes() {
    let src = program("const o: any = { f: globalThis.structuredClone, p }; return o.f(u);");
    let pf = facts(&src);
    assert!(
        !proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// Control: what the call passes still reaches unknown code.
#[test]
fn an_argument_of_the_bare_global_call_still_escapes() {
    let src = program("return fetch(u, { body: p } as any);");
    let pf = facts(&src);
    assert!(
        !proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}

/// Control: a declared global function runs its declaration, which names
/// no receiver (node's `fetch(input, init)` never reads `this`): a program
/// object it is called on stays the program's, what `init.body` holds
/// reaches unknown code (`escapes`).
#[test]
fn a_declared_global_function_gives_its_receiver_to_no_one() {
    let src = program("const o: any = { f: fetch, p }; return o.f(u);");
    let pf = facts(&src);
    assert!(
        proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
    let src = program("const o: any = { f: fetch }; return o.f(u, { body: p });");
    let pf = facts(&src);
    assert!(
        !proven(&pf, &src, "// SITE", "x"),
        "{}",
        verdicts(&pf, &src, "// SITE", "x")
    );
}
