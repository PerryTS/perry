//! A store onto the global object's `navigator` (Node 26: `get navigator`,
//! no setter) meets an accessor without a setter: [[Set]] writes nothing
//! (sloppy) or throws a TypeError (strict). The store is no native write, and
//! its value never reaches the load (`builtins::getter_only`), until a native
//! write may have replaced the accessor.
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

const SKINBENCH: &str = "const g: any = typeof globalThis !== \"undefined\" ? globalThis : {};\n\
     if (typeof navigator === \"undefined\") g.navigator = { userAgent: true };";

/// The skinbench shape: a `typeof`-guarded store of a literal. The load reads
/// the realm's navigator only: no ⊤, and not the literal's `true`.
#[test]
fn store_onto_getter_only_navigator_writes_nothing() {
    for prelude in [
        SKINBENCH,
        "(globalThis as any).navigator = { userAgent: true };",
    ] {
        for read in [
            "navigator.userAgent",
            "(globalThis as any).navigator.userAgent",
        ] {
            let k = stored(prelude, read);
            assert!(!k.top, "{prelude} / {read}: {k:?}");
            assert_eq!(
                k.prims & !(prim::NUM | prim::UNDEF),
                prim::STR,
                "{prelude} / {read}: {k:?}"
            );
        }
    }
}

/// The seed: after the store, `userAgent.match` hands nothing to unknown
/// code; a later object read stays proven.
#[test]
fn store_onto_navigator_then_match_keeps_sites_proven() {
    let pf = facts(&format!(
        "{SKINBENCH}\n\
         class O {{ x = 1; }}\n\
         const o = new O();\n\
         const r: any = /a/;\n\
         r.o = o;\n\
         const m = navigator.userAgent.match(r);\n\
         console.log(m, o.x);\n"
    ));
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert!(
        !s.is_empty() && s.iter().all(|s| !matches!(s.verdict, Verdict::No(_))),
        "{}",
        pf.dump_text()
    );
}

/// Control: `Object.defineProperty` replaces the accessor (a native write
/// under the key): the load reads ⊤, and the store, replayed on the ordinary
/// path, reaches it as before.
#[test]
fn navigator_redefined_then_stored_reads_top() {
    let k = stored(
        &format!(
            "Object.defineProperty(globalThis, \"navigator\", {{ value: {{ userAgent: 1 }} }});\n{SKINBENCH}"
        ),
        "navigator.userAgent",
    );
    assert!(k.top, "{k:?}");
}

/// The predicate is the realm's: a getter and no setter (`navigator`,
/// `crypto`); an accessor with a setter (`localStorage`) or a data
/// property is not getter-only.
#[test]
fn getter_only_reads_the_realm_accessor() {
    let m = crate::model::Model::default();
    let declared = |k: &str| m.namespace_property(crate::model::BuiltinNs::GlobalThis, k);
    let only = |k: &str| declared(k).is_some_and(|p| p.get.is_some() && p.set.is_none());
    assert!(only("navigator") && only("crypto"));
    assert!(declared("localStorage").is_some_and(|p| p.get.is_some() && p.set.is_some()));
    assert!(!only("Array") && !only("__proto__"));
}
