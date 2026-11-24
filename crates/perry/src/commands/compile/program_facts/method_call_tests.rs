//! Certification and publication share one decision, including refusals.
use super::*;

const SOURCE: &str =
    "class P { m() { return 7; } run() { return this.m(); } } console.log(new P().run());";

fn module(source: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(source, "main.ts").unwrap();
    perry_hir::lower_module(&ast, "main", "main.ts").unwrap()
}

fn facts(hir: &perry_hir::Module) -> perry_wpfacts::ProgramFacts {
    perry_wpfacts::compute(
        &[("main.ts".into(), hir)],
        &perry_wpfacts::FactsConfig {
            namespace_pairs: namespace_pairs_on(),
            ..Default::default()
        },
    )
}

fn published(
    pf: &perry_wpfacts::ProgramFacts,
    hir: &perry_hir::Module,
) -> perry_codegen::ElisionFacts {
    let out = elision_for(pf, "main.ts", &|path, class, function, key, member| {
        assert_eq!(path, "main.ts");
        assert_eq!(member, Member::Method);
        method_symbol_in(hir, "main", class, function, key)
    });
    for s in pf.sites.iter().filter(|s| s.is_call) {
        assert_eq!(
            s.method_call_certificate().is_some(),
            out.method_calls
                .contains_key(&(s.byte_offset, s.key.clone()))
        );
    }
    out
}

#[test]
fn method_call_publication_keeps_dispatch_scope_narrow() {
    let hir = module(SOURCE);
    let pf = facts(&hir);
    let out = published(&pf, &hir);
    for (key, redispatch) in [("m", false), ("run", true)] {
        let s = pf.sites.iter().find(|s| s.is_call && s.key == key).unwrap();
        assert!(out.method_calls.contains_key(&(s.byte_offset, key.into())));
        assert_eq!(s.dispatch_obligation().unwrap().redispatch, redispatch);
    }
}

#[test]
fn method_call_publication_refuses_an_ineligible_twin() {
    let mut hir = module(&format!("{SOURCE} class Q {{ async m() {{ return 8; }} run() {{ return this.m(); }} }} new Q().run();"));
    let pf = facts(&hir);
    let m: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.key == "m")
        .collect();
    assert_eq!(m.len(), 2);
    assert_eq!(
        m.iter()
            .filter(|s| s.method_call_certificate().is_some())
            .count(),
        1
    );
    let offset = m[0].byte_offset;
    fn same_offset(e: &mut perry_hir::Expr, offset: u32) {
        if let perry_hir::Expr::PropertyGet {
            property,
            byte_offset,
            ..
        } = e
        {
            if property == "m" {
                *byte_offset = offset;
            }
        }
        perry_hir::walker::walk_expr_children_mut(e, &mut |e| same_offset(e, offset));
    }
    for c in &mut hir.classes {
        for f in &mut c.methods {
            for st in &mut f.body {
                if let perry_hir::Stmt::Return(Some(e), _) = st {
                    same_offset(e, offset);
                }
            }
        }
    }
    let pf = facts(&hir);
    let out = published(&pf, &hir);
    let m: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.is_call && s.key == "m")
        .collect();
    assert!(!m.is_empty());
    assert!(m
        .iter()
        .all(|s| s.byte_offset == offset && s.method_call_certificate().is_none()));
    assert!(m
        .iter()
        .all(|s| s.dispatch_obligation().unwrap().redispatch));
    assert!(!out.method_calls.contains_key(&(offset, "m".into())));
}

#[test]
fn method_call_publication_respects_elide_off() {
    // An isolated process keeps this env control independent of parallel tests.
    if elide_enabled() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "commands::compile::program_facts::method_call_tests::method_call_publication_respects_elide_off", "--nocapture"])
            .env("PERRY_FACTS_ELIDE", "0").output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{output:?}"
        );
        return;
    }
    assert!(!elide_enabled());
    let hir = module(SOURCE);
    let pf = facts(&hir);
    let s = pf.sites.iter().find(|s| s.is_call && s.key == "m").unwrap();
    assert!(matches!(
        s.verdict,
        perry_wpfacts::Verdict::T0Mono | perry_wpfacts::Verdict::T0Set
    ));
    assert!(s.method_call_certificate().is_none());
    assert!(s.dispatch_obligation().unwrap().redispatch);
    assert!(published(&pf, &hir).method_calls.is_empty());
}

#[test]
fn native_dispatch_builtin_certificate_publishes_the_intrinsic_selection() {
    for (source, certified) in [
        ("Math.abs(-3);", true),
        ("console.log('hello');", false),
        ("console.log({});", false),
        (
            "console.log = function(x: any) {}; console.log('hello');",
            false,
        ),
    ] {
        let mut patched = perry_hir::PatchedBuiltins::new();
        patched.namespace_pairs = namespace_pairs_on();
        perry_hir::set_patched_builtins(std::sync::Arc::new(patched));
        let hir = module(source);
        perry_hir::clear_patched_builtins();
        let pf = facts(&hir);
        let out = published(&pf, &hir);
        let site = pf
            .sites
            .iter()
            .find(|s| s.is_call && s.key == if certified { "abs" } else { "log" })
            .unwrap();
        assert_eq!(
            site.builtin_dispatch_certified(),
            certified,
            "{}",
            pf.dump_text()
        );
        if certified {
            assert!(perry_hir::namespace_pairs::intrinsic_selected(
                site.byte_offset,
                &out.native_member_calls
            ));
        }
    }
}
