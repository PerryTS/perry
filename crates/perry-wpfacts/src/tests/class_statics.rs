//! FVA 3.4: a class object's statics are its own properties, created when
//! the class is evaluated. `C.k` is proven when k is one of C's statics,
//! nothing else stores it, and the class object reaches no unknown code.

use super::*;

fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    pf.sites.iter().filter(|s| s.key == key).collect()
}

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s = sites(pf, key);
    !s.is_empty()
        && s.iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

fn units(src: &str) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default())
}

/// The class `Pk` is reached through a value (`k`), so its member reads and
/// calls are sites on the class object.
const PK: &str = r#"
    class Pk { static of(n: number) { return n * 3; } static x = 5; }
    function useIt(k: any) { return k.of(2) + k.x; }
    console.log(useIt(Pk));
"#;

#[test]
fn a_constant_static_called_through_a_value_is_proven() {
    let pf = facts(PK);
    // `prototype` is one too: a class's is neither writable nor configurable.
    assert_eq!(
        tmpl(&pf, "Pk").statics,
        ["of", "prototype", "x"],
        "{}",
        pf.dump_text()
    );
    assert!(proven(&pf, "of"), "{}", pf.dump_text());
    assert!(proven(&pf, "x"), "{}", pf.dump_text());
    let of = sites(&pf, "of");
    assert!(
        of[0].targets.iter().any(|t| t.contains(":of#")),
        "{:?}",
        of[0]
    );
    // Per unit claims no more than whole program.
    let u = units(PK);
    for k in ["of", "x"] {
        if proven(&u, k) {
            assert!(proven(&pf, k));
        }
    }
}

#[test]
fn a_nested_class_static_is_defined_where_the_class_is_evaluated() {
    // A class inside a function (as every class of a CommonJS module is):
    // its static fields are stored where it is evaluated, once each.
    let pf = facts(
        r#"
        function mk() { class N { static t = 5; static of() { return N.t; } } return N; }
        function use(k: any) { return k.of() + k.t; }
        console.log(use(mk()));
        "#,
    );
    assert_eq!(
        tmpl(&pf, "N").statics,
        ["of", "prototype", "t"],
        "{}",
        pf.dump_text()
    );
    assert!(proven(&pf, "of") && proven(&pf, "t"), "{}", pf.dump_text());
}

#[test]
fn a_static_method_stored_after_its_definition_is_not_constant() {
    let src = PK.replace(
        "console.log(useIt(Pk));",
        "console.log(useIt(Pk)); (Pk as any).of = (n: number) => n; console.log(useIt(Pk));",
    );
    let pf = facts(&src);
    assert_eq!(
        tmpl(&pf, "Pk").statics,
        ["prototype", "x"],
        "{}",
        pf.dump_text()
    );
    assert!(
        sites(&pf, "of")
            .iter()
            .all(|s| s.verdict == Verdict::No("not a constant static of the class (3.4)".into())),
        "{}",
        pf.dump_text()
    );
    assert!(proven(&pf, "x"), "{}", pf.dump_text());
}

#[test]
fn a_class_reaching_unknown_code_or_deleted_from_has_no_constant_statics() {
    for extra in ["(globalThis as any).hook?.(Pk);", "delete (Pk as any).x;"] {
        let src = PK.replace(
            "console.log(useIt(Pk));",
            &format!("{extra} console.log(useIt(Pk));"),
        );
        let pf = facts(&src);
        assert!(
            tmpl(&pf, "Pk").statics.is_empty(),
            "{extra}\n{}",
            pf.dump_text()
        );
        assert!(!proven(&pf, "of"), "{extra}\n{}", pf.dump_text());
    }
}

#[test]
fn a_computed_static_member_or_a_static_accessor_is_not_constant() {
    let pf = facts(
        r#"
        const n = "c" + String(1);
        class Q { static of() { return 1; } static [n]() { return 2; } }
        class R { static of() { return 3; } static get g() { return 4; } }
        function use(q: any, r: any) { return q.of() + r.of() + r.g; }
        console.log(use(Q, R));
        "#,
    );
    assert!(tmpl(&pf, "Q").statics.is_empty(), "{}", pf.dump_text());
    let r = &tmpl(&pf, "R").statics;
    assert!(
        r.contains(&"of".to_string()) && !r.contains(&"g".to_string()),
        "{r:?}"
    );
}
