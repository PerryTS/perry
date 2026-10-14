//! Unit tests: small programs lowered from source, facts checked directly.

use crate::{compute, FactsConfig, ProgramFacts, Verdict};

fn facts(src: &str) -> ProgramFacts {
    facts_with(src, FactsConfig::default())
}

fn facts_with(src: &str, cfg: FactsConfig) -> ProgramFacts {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    compute(&[("t.ts".to_string(), &hir)], &cfg)
}

fn tmpl<'a>(pf: &'a ProgramFacts, name: &str) -> &'a crate::TemplateFacts {
    pf.templates
        .iter()
        .find(|t| t.display.ends_with(&format!(":{name}")))
        .unwrap_or_else(|| panic!("template {name} in\n{}", pf.dump_text()))
}

fn field<'a>(t: &'a crate::TemplateFacts, k: &str) -> &'a crate::FieldFacts {
    t.fields.iter().find(|f| f.key == k).expect("field")
}

#[test]
fn constructor_only_field_is_readonly_definite_and_t0() {
    let pf = facts(
        r#"
        class P { x: number; constructor(x: number) { this.x = x; } }
        function get(p: P) { return p.x; }
        console.log(get(new P(1)));
        "#,
    );
    let p = tmpl(&pf, "P");
    assert!(p.prefix_stable, "{}", pf.dump_text());
    assert!(p.fixed_layout, "{}", pf.dump_text());
    let x = field(p, "x");
    assert!(x.readonly && x.definite);
    assert_eq!(x.kind.desc(), "num");
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn post_construction_store_kills_readonly() {
    let pf = facts(
        r#"
        class P { x: number; constructor() { this.x = 1; } bump() { this.x = 2; } }
        const p = new P(); p.bump(); console.log(p.x);
        "#,
    );
    let p = tmpl(&pf, "P");
    assert!(!field(p, "x").readonly, "{}", pf.dump_text());
    assert!(p.prefix_stable);
}

#[test]
fn delete_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; y = 2; }
        const p = new P(); delete (p as any).x; console.log(p.y);
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn define_property_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const p = new P(); Object.defineProperty(p, "x", { get() { return 5; } });
        console.log(p.x);
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn any_alias_store_widens_kind() {
    let pf = facts(
        r#"
        class P { x: number = 1; }
        const p = new P(); const a: any = p; a.x = "s"; console.log(p.x);
        "#,
    );
    let x = field(tmpl(&pf, "P"), "x");
    assert!(!x.readonly);
    assert!(x.kind.desc().contains("str"), "{}", pf.dump_text());
}

#[test]
fn object_create_of_prototype_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; m() { return this.x; } }
        const o = Object.create(P.prototype); console.log(o.m(), new P().m());
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn set_prototype_of_is_hard() {
    let pf = facts(
        r#"
        class P { x = 1; }
        class Q { y = 2; }
        const p = new P(); Object.setPrototypeOf(p, Q.prototype); console.log(p.x);
        "#,
    );
    assert!(!tmpl(&pf, "P").prefix_stable, "{}", pf.dump_text());
}

#[test]
fn closure_writer_is_post_construction() {
    let pf = facts(
        r#"
        class P { x = 1; }
        const p = new P();
        const set = () => { p.x = 3; };
        set(); console.log(p.x);
        "#,
    );
    assert!(!field(tmpl(&pf, "P"), "x").readonly, "{}", pf.dump_text());
}

#[test]
fn unknown_callee_escapes_argument() {
    let pf = facts(
        r#"
        class P { x = 1; }
        declare const ext: any;
        const p = new P(); ext(p); console.log(p.x);
        "#,
    );
    let p = tmpl(&pf, "P");
    assert!(p.escape.is_some() && !p.prefix_stable, "{}", pf.dump_text());
}

#[test]
fn method_call_resolves_one_target() {
    let pf = facts(
        r#"
        class P { x = 1; m() { return this.x; } }
        function f(p: P) { return p.m(); }
        console.log(f(new P()));
        "#,
    );
    let site = pf
        .sites
        .iter()
        .find(|s| s.key == "m" && s.is_call)
        .expect("call site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn literal_template_facts() {
    let pf = facts(
        r#"
        function mk(n: number) { return { a: n, b: "s" }; }
        function rd(o: { a: number; b: string }) { return o.a; }
        console.log(rd(mk(1)));
        "#,
    );
    let site = pf.sites.iter().find(|s| s.key == "a").expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn sabotage_plants_the_fact() {
    let src = r#"
        class P { x: number = 1; bump() { this.x++; } }
        const p = new P(); p.bump(); console.log(p.x);
    "#;
    let pf = facts(src);
    assert!(!field(tmpl(&pf, "P"), "x").readonly);
    let pf2 = facts_with(
        src,
        FactsConfig {
            relations: false,
            sabotage: Some("readonly:P.x".into()),
            dynamic_code: None,
            budget_secs: None,
            whatif_no_e14: false,
            profile: false,
            max_pts: None,
            widen_k: None,
            ..Default::default()
        },
    );
    assert!(field(tmpl(&pf2, "P"), "x").readonly);
    assert!(pf2.dump_text().contains("# sabotage readonly:P.x"));
    assert_ne!(pf.digest_for("t.ts"), pf2.digest_for("t.ts"));
}

/// Writes `<dir>/t.facts` and `<dir>/t.facts.rel` for the checker scripts
/// when `WPFACTS_TEST_DUMP_DIR` is set (developer aid; a no-op otherwise).
#[test]
fn write_dump_for_checker() {
    let Some(dir) = std::env::var_os("WPFACTS_TEST_DUMP_DIR") else {
        return;
    };
    let src = std::env::var("WPFACTS_TEST_SRC").unwrap_or_else(|_| {
        r#"
        class P { x: number; y = "s"; constructor(x: number) { this.x = x; } m() { return this.x; } }
        class Q extends P { z = 1; get g() { return this.z; } }
        function f(p: P) { return p.x + p.m(); }
        const o = { a: 1, b: [1, 2] };
        const q = new Q(2);
        q.z = 3;
        console.log(f(new P(1)), f(q), q.g, o.a, o.b.length);
        "#
        .to_string()
    });
    let pf = facts_with(
        &src,
        FactsConfig {
            relations: true,
            sabotage: std::env::var("WPFACTS_TEST_SABOTAGE").ok(),
            dynamic_code: None,
            budget_secs: None,
            whatif_no_e14: false,
            profile: false,
            max_pts: None,
            widen_k: None,
            ..Default::default()
        },
    );
    let dir = std::path::PathBuf::from(dir);
    std::fs::write(dir.join("t.facts"), pf.dump_text()).unwrap();
    std::fs::write(dir.join("t.facts.rel"), pf.relations_text().unwrap()).unwrap();
}

#[test]
fn dynamic_code_reach_kills_facts_without_an_unknown_call() {
    let mut reach = perry_hir::dynamic_code::DynamicCodeReach::default();
    reach.record_global("new Function(...) @ t.ts:4".into());
    let pf = facts_with(
        "class P { x = 1; m() { return this.x; } } console.log(new P().m());",
        FactsConfig {
            dynamic_code_reach: reach,
            relations: true,
            ..Default::default()
        },
    );
    assert!(pf.dynamic_code_reach.disables_builtin_constants());
    assert!(pf.relations_text().unwrap().contains("dynamicreach 1"));
    assert!(pf.global.as_deref().unwrap().contains("dynamic_code_reach"));
    assert!(!tmpl(&pf, "P").prefix_stable);
    assert!(!field(tmpl(&pf, "P"), "x").readonly);
}
