//! A store into ⊤ (a parameter of a function handed to unknown code,
//! written under a computed key) writes a built-in object only once that
//! object was handed to unknown code (`SharedWrite::Handed`), or the global
//! object was (`Model::global_in_top`: unknown code then reaches every
//! built-in by name). Interpreted code holds the global object
//! (`Function('return this')()`), so dynamic code that may run hands it.
//! node: with nothing handed, `unknown.take` cannot reach `console`,
//! `String.prototype` or `Object`; each program below prints what the
//! model predicts.

use super::*;

const PRE: &str = r#"
    declare const k: number;
    class A { zz = 1; }
    class M { m(o: any) { return o.zz; } }
    class N { inner: any = new M(); }
    const unknown: any = (globalThis as any)[process.argv[2]];
    unknown.take(function (arr: any, kk: any) { arr[kk] = 1; });
"#;

/// A built-in member lookup past the store into ⊤, the site it decides
/// (`key`) and the expression that hands the built-in object itself.
struct Case {
    what: &'static str,
    body: String,
    key: &'static str,
    hand: &'static str,
}

fn cases() -> Vec<Case> {
    // A key no intrinsic defines, looked up past `v`'s built-in chain:
    // `i.m` is proven while that key is not written.
    let inner = |other: &str| {
        format!(
            "const v: any = k > 1 ? new N() : {other}; const i = v.inner; if (i) console.log(i.m(new A()));"
        )
    };
    vec![
        Case {
            what: "console.warn",
            body: "const o: A = new A(); console.warn(o); console.log(o.zz);".into(),
            key: "zz",
            hand: "console",
        },
        Case {
            what: "String.prototype",
            body: inner("\"text\""),
            key: "m",
            hand: "String.prototype",
        },
        Case {
            what: "Object",
            body: inner("Object"),
            key: "m",
            hand: "Object",
        },
    ]
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn check(cfg: Option<fn() -> FactsConfig>, before: &str, after: &str, want: bool) {
    for c in cases() {
        let src = format!("{PRE}{before}\n{}\n{after}", c.body);
        let pf = match &cfg {
            Some(cfg) => facts_with(&src, cfg()),
            None => facts(&src),
        };
        assert_eq!(proven(&pf, c.key), want, "{}: {src}\n{}", c.what, pf.dump_text());
    }
}

/// Nothing handed: the store into ⊤ writes none of them. (Was: every
/// built-in object but the global one written, `console.warn` an unknown
/// method that `o` escaped to.)
#[test]
fn a_top_store_writes_no_builtin_object_nothing_handed() {
    check(None, "", "", true);
}

/// The object handed to unknown code before the lookup: written.
#[test]
fn a_builtin_object_handed_first_is_written_by_a_top_store() {
    for c in cases() {
        let hand = format!("unknown.take2({});", c.hand);
        let src = format!("{PRE}{hand}\n{}", c.body);
        let pf = facts(&src);
        assert!(!proven(&pf, c.key), "{}: {src}\n{}", c.what, pf.dump_text());
    }
}

/// Handed after the lookup was solved: the parked lookup runs again
/// (`note_handed_to_unknown`).
#[test]
fn a_builtin_object_handed_after_the_lookup_is_written_by_a_top_store() {
    for c in cases() {
        let hand = format!("unknown.take2({});", c.hand);
        let src = format!("{PRE}{}\n{hand}", c.body);
        let pf = facts(&src);
        assert!(!proven(&pf, c.key), "{}: {src}\n{}", c.what, pf.dump_text());
    }
}

/// The global object handed: every built-in object is in reach by name.
#[test]
fn the_global_object_handed_puts_every_builtin_in_reach_of_a_top_store() {
    check(None, "unknown.take2(globalThis);", "", false);
    check(None, "", "unknown.take2(globalThis);", false);
}

/// Dynamic code that may run holds the global object
/// (`interpreted_code_runs` sets `global_in_top`): each source alone.
#[test]
fn dynamic_code_puts_every_builtin_in_reach_of_a_top_store() {
    for d in [
        "new Function(process.argv[4])();",
        "eval(process.argv[4]);",
        "(0, eval)(process.argv[4]);",
    ] {
        check(Some(interp_cfg), d, "", false);
        check(Some(interp_cfg), "", d, false);
    }
}

/// Control: the interpreter linked, no dynamic code reached: nothing
/// hands the global object.
#[test]
fn a_linked_interpreter_without_dynamic_code_hands_nothing() {
    check(Some(interp_cfg), "", "", true);
}

/// `Function('return this')()` folds to the global object (HIR
/// `bare_builtins`): compiled code holds it, which hands nothing; handed
/// to unknown code, every built-in object is in reach.
#[test]
fn the_folded_global_object_is_handed_only_when_given() {
    check(None, "Function('return this')();", "", true);
    check(None, "unknown.take2(Function('return this')());", "", false);
}

/// A sloppy function unknown code calls with a nullish `this` sees the
/// global object (`sloppy_this`); handing that on hands the global object.
#[test]
fn a_sloppy_this_handed_on_puts_every_builtin_in_reach() {
    check(None, "unknown.take3(function (this: any) { unknown.take2(this); });", "", false);
}
