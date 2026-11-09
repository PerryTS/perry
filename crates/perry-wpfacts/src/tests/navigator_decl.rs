//! `navigator` is the realm's one object (runtime navigator.rs: namespace
//! `navigator`, [[Prototype]] `Navigator.prototype`), which Node's global
//! getter returns: a load of it is that object unless something other than
//! a program store may have written the global key (`global_value_written`).
//! Its getters declare their result types (`userAgent`: a string).
use super::*;
use crate::model::prim;

/// The kind of `H.v`, after `h.v = <read>` (`prelude` runs first).
fn stored(prelude: &str, read: &str) -> crate::derive::KindFact {
    let pf = facts(&format!(
        "class H {{ v: any = 0; }}\n\
         {prelude}\n\
         const h = new H();\n\
         h.v = {read};\n\
         console.log(h.v);\n"
    ));
    field(tmpl(&pf, "H"), "v").kind.clone()
}

#[test]
fn navigator_user_agent_is_the_declared_string() {
    for read in ["navigator.userAgent", "(globalThis as any).navigator.userAgent"] {
        let k = stored("", read);
        assert!(!k.top, "{read}: {k:?}");
        // `H.v` starts as a number; the read adds a string (and `undefined`,
        // which the model keeps for the global read), nothing else.
        assert_eq!(k.prims & !(prim::NUM | prim::UNDEF), prim::STR, "{read}: {k:?}");
    }
    let k = stored("", "navigator.hardwareConcurrency");
    assert!(!k.top && k.prims & !prim::UNDEF == prim::NUM, "{k:?}");
}

/// The seed: a RegExp handed to `userAgent.match` stays with the string
/// method; a later object read stays proven.
#[test]
fn navigator_user_agent_match_hands_nothing_to_unknown_code() {
    let pf = facts(
        "class O { x = 1; }\n\
         const o = new O();\n\
         const r: any = /x/;\n\
         r.o = o;\n\
         const m = navigator.userAgent.match(r);\n\
         console.log(m, o.x);\n",
    );
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert!(!s.is_empty() && s.iter().all(|s| !matches!(s.verdict, Verdict::No(_))), "{}", pf.dump_text());
}

/// Control: a write the program does not make as a store (here
/// `Object.defineProperty` on the global object) keeps the read ⊤.
#[test]
fn navigator_redefined_on_the_global_reads_top() {
    let k = stored(
        "Object.defineProperty(globalThis, \"navigator\", { value: { userAgent: 1 } });",
        "navigator.userAgent",
    );
    assert!(k.top, "{k:?}");
}
