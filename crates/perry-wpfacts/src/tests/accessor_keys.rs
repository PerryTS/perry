//! FVA 4.3a: `Object.defineProperty(o, k, { get/set })` with a constant key
//! and an accessor descriptor is an accessor key k on o's layout holding the
//! getter, not E5 HARD, when the layout does not hold k as data. A key it
//! holds as data, a class instance, an unknown key or a data descriptor
//! stays E5.

use super::*;

fn site<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    pf.sites.iter().filter(|s| s.key == key).collect()
}

/// The literal layout holding birth key `key`.
fn lit<'a>(pf: &'a ProgramFacts, key: &str) -> &'a crate::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| t.kind == "litclass" && t.birth_keys == [key])
        .unwrap_or_else(|| panic!("a literal with birth key {key}\n{}", pf.dump_text()))
}

#[test]
fn a_getter_on_a_new_key_is_an_accessor_key_not_hard() {
    let pf = facts(
        r#"
        class A { a = 1; }
        const ns: any = { tag: 1 };
        Object.defineProperty(ns, "k", { get: () => new A(), enumerable: true });
        function rd(o: any) { return o.k.a + o.tag; }
        console.log(rd(ns));
        "#,
    );
    let t = lit(&pf, "tag");
    assert!(t.hard.is_none() && t.prefix_stable, "{}", pf.dump_text());
    let k = t
        .fields
        .iter()
        .find(|f| f.key == "k")
        .expect("k on the layout");
    assert!(!k.data_field, "{k:?}");
    // The other key is still one load; the getter runs on the read of k.
    assert!(
        site(&pf, "tag")
            .iter()
            .all(|s| matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set)),
        "{}",
        pf.dump_text()
    );
    assert!(
        site(&pf, "a")
            .iter()
            .all(|s| s.receivers.iter().any(|r| r.ends_with(":A"))),
        "{}",
        pf.dump_text()
    );
    assert!(
        site(&pf, "k")
            .iter()
            .all(|s| s.verdict == Verdict::No("accessor key (E25)".into())),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_key_the_layout_holds_as_data_stays_hard() {
    let pf = facts(
        r#"
        const m: any = { tag: 2 };
        Object.defineProperty(m, "tag", { get: () => 7 });
        function rd(o: any) { return o.tag; }
        console.log(rd(m));
        "#,
    );
    let t = lit(&pf, "tag");
    assert!(
        t.hard.as_deref().is_some_and(|h| h.starts_with("E5")),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_data_descriptor_or_an_unknown_key_stays_hard() {
    for define in [
        r#"Object.defineProperty(m, "v", { value: 1 });"#,
        r#"Object.defineProperty(m, String(Math.random()), { get: () => 1 });"#,
    ] {
        let pf = facts(&format!(
            "const m: any = {{ tag: 2 }}; {define} function rd(o: any) {{ return o.tag; }} console.log(rd(m));"
        ));
        let t = lit(&pf, "tag");
        assert!(t.hard.is_some(), "{define}\n{}", pf.dump_text());
    }
}

#[test]
fn another_units_layout_stays_hard_per_unit() {
    // Per unit, the statement defining the getter holds `m` as a stand-in of
    // the declaring unit's layout: an accessor its owner does not describe
    // is beyond the contract, so the census breaks the layout (readers see
    // TOP), as whole program refuses it.
    let src = r#"
        const m: any = { tag: 2 };
        Object.defineProperty(m, "tag", { get: () => 7 });
        function rd(o: any) { return o.tag; }
        console.log(rd(m));
    "#;
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let u = crate::compute_units(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    let d = lit(&u, "tag").display.clone();
    assert!(u.broken.contains(&d), "{:?}\n{}", u.broken, u.dump_text());
}
