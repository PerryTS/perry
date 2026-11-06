//! Body-derived own-test helpers: each assertion names the guarded value's
//! `size` call site, so a prototype value leaking through the read fails it.
use super::*;

const HELPER: &str = "function guard(map: any, key: any) { return h.call(map, key); }";

fn source(helper: &str, body: &str) -> String {
    format!(
        r#"
        const h = Object.prototype.hasOwnProperty;
        class Sym {{ n = 1; size() {{ return this.n; }} }}
        const dict: any = {{}};
        for (const n of ["a", "b"]) dict[n] = new Sym();
        let total = 0;
        function other() {{ total += 2; }}
        {helper}
        for (const k of ["a", "x", "constructor"]) {{ {body} }}
        console.log(total);
    "#
    )
}

fn verdict(src: &str, expected: bool) {
    verdict_facts(facts(src), expected);
}

fn verdict_facts(pf: ProgramFacts, expected: bool) {
    let sites: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "size" && s.is_call)
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "size must name one live site: {}",
        pf.dump_text()
    );
    assert_eq!(
        !matches!(sites[0].verdict, Verdict::No(_)),
        expected,
        "size: {:?}; receivers={:?}",
        sites[0].verdict,
        sites[0].receivers
    );
}

#[test]
fn helper_and_site_proven() {
    verdict(
        &source(
            HELPER,
            "if (guard(dict, k) && dict[k].size() > 0) total += 1;",
        ),
        true,
    );
}

#[test]
fn helper_conditional_site_proven() {
    verdict(
        &source(HELPER, "total += guard(dict, k) ? dict[k].size() : 0;"),
        true,
    );
}

#[test]
fn helper_negated_or_site_proven() {
    verdict(
        &source(
            HELPER,
            "if (!guard(dict, k) || dict[k].size() > 0) total += 1;",
        ),
        true,
    );
}

#[test]
fn helper_summary_body_site_proven() {
    let src = source(&format!("{HELPER}\nfunction get(map: any, key: any) {{ return guard(map, key) ? map[key] : undefined; }}"),
        "const s = get(dict, k); if (s !== undefined) total += s.size();");
    verdict(&src, true);
    let ast = perry_parser::parse_typescript(&src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let (g, _, _) = crate::generate(
        &[("t.ts", &hir)],
        &FactsConfig::default(),
        None,
        None,
        None,
        std::time::Instant::now(),
    );
    let get =
        g.m.funcs
            .iter()
            .position(|f| f.name.ends_with(":get"))
            .unwrap() as u32;
    assert!(
        g.m.sum.of.contains_key(&get),
        "get must actually be summarized"
    );
    assert!(
        g.m.own_read_census[0] > 0,
        "mapped summary reads must be counted"
    );
}

#[test]
fn helper_object_has_own_site_proven() {
    verdict(
        &source(
            "function guard(map: any, key: any) { return Object.hasOwn(map, key); }",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        true,
    );
}

#[test]
fn helper_value_alias_site_proven() {
    verdict(
        &source(
            &format!("{HELPER}\nconst alias = guard;"),
            "total += alias(dict, k) ? dict[k].size() : 0;",
        ),
        true,
    );
}

#[test]
fn helper_closure_site_proven() {
    verdict(
        &source(
            "const guard = (map: any, key: any) => h.call(map, key);",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        true,
    );
}

#[test]
fn helper_second_return_unproven() {
    verdict(&source("function guard(map: any, key: any) { if (total > 5) return true; return h.call(map, key); }",
        "total += guard(dict, k) ? dict[k].size() : 0;"), false);
}

#[test]
fn helper_other_operands_unproven() {
    verdict(
        &source(
            "function guard(map: any, key: any) { return h.call(dict, key); }",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_other_parameter_key_unproven() {
    verdict(
        &source(
            "function guard(map: any, key: any, other: any) { return h.call(map, other); }",
            "total += guard(dict, k, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_binding_reassigned_unproven() {
    verdict(
        &source(
            "let guard: any = (map: any, key: any) => h.call(map, key); guard = () => true;",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_mixed_callee_unproven() {
    verdict(
        &source(
            &format!("{HELPER}\nlet alias: any = guard; if (total > 5) alias = () => true;"),
            "total += alias(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_h_replaced_unproven() {
    verdict(&source("let hh: any = h; function guard(map: any, key: any) { return hh.call(map, key); } hh = () => true;",
        "total += guard(dict, k) ? dict[k].size() : 0;"), false);
}

#[test]
fn helper_h_call_replaced_unproven() {
    verdict(
        &source(
            &format!("{HELPER}\n(h as any).call = () => true;"),
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_object_has_own_replaced_unproven() {
    verdict(&source("function guard(map: any, key: any) { return Object.hasOwn(map, key); } (Object as any).hasOwn = () => true;",
        "total += guard(dict, k) ? dict[k].size() : 0;"), false);
}

#[test]
fn helper_statement_between_unproven() {
    verdict(
        &source(
            HELPER,
            "if (guard(dict, k)) { other(); total += dict[k].size(); }",
        ),
        false,
    );
}

#[test]
fn helper_different_object_unproven() {
    verdict(
        &source(
            HELPER,
            "const d: any = {}; total += guard(d, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_different_key_unproven() {
    verdict(
        &source(
            HELPER,
            "const kk = k + 'x'; total += guard(dict, kk) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_parameter_reassigned_unproven() {
    verdict(
        &source(
            "function guard(map: any, key: any) { key = 'a'; return h.call(map, key); }",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_after_test_side_effect_unproven() {
    verdict(&source("function guard(map: any, key: any) { const yes = h.call(map, key); delete map[key]; return yes; }",
        "total += guard(dict, k) ? dict[k].size() : 0;"), false);
}

#[test]
fn helper_summary_replaced_test_unproven() {
    verdict(&source(&format!("{HELPER}\nfunction get(map: any, key: any) {{ return guard(map, key) ? map[key] : undefined; }} (h as any).call = () => true;"),
        "const s = get(dict, k); if (s !== undefined) total += s.size();"), false);
}

#[test]
fn helper_default_parameter_unproven() {
    verdict(
        &source(
            "function guard(map: any, key: any = 'a') { return h.call(map, key); }",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_async_unproven() {
    verdict(
        &source(
            "async function guard(map: any, key: any) { return h.call(map, key); }",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_declared_unproven() {
    verdict(
        &source(
            "declare function guard(map: any, key: any): boolean;",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_summary_mixed_callee_unproven() {
    verdict(&source(&format!("{HELPER}\nfunction get(test: any, map: any, key: any) {{ return test(map, key) ? map[key] : undefined; }}"),
        "let test: any = guard; if (total > 5) test = () => true; const s = get(test, dict, k); if (s !== undefined) total += s.size();"), false);
}

#[test]
fn helper_generator_unproven() {
    verdict(
        &source(
            "function* guard(map: any, key: any) { return h.call(map, key); }",
            "total += guard(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

#[test]
fn helper_bound_callee_unproven() {
    verdict(
        &source(
            &format!("{HELPER}\nconst test = guard.bind(undefined);"),
            "total += test(dict, k) ? dict[k].size() : 0;",
        ),
        false,
    );
}

/// Control evidence cannot contaminate a value copy or the leak metric.
#[test]
fn refutation_marker_does_not_flow_as_a_value() {
    use crate::model::{Cons, Model, OwnGateAt};
    let mut m = Model::default();
    let p = m.node_l("predicate");
    let value = m.node_l("value");
    let q = m.node_l("predicate");
    m.edge(p, value);
    m.refute_own_test(p);
    // Attaching a predicate relay after refutation must replay the marker.
    m.cons(
        p,
        Cons::OwnTest {
            dst: q,
            at: OwnGateAt::Refuted,
        },
    );
    assert!(m.nodes[p as usize].own_refuted && m.nodes[q as usize].own_refuted);
    assert!(!m.nodes[value as usize].own_refuted);
    for n in [p, q, value] {
        assert!(m.nodes[n as usize].pts.is_empty());
        assert_eq!(m.nodes[n as usize].prim, 0);
    }
}

fn tdz_source(helper: &str) -> String {
    format!(
        "const early = guard;\n{}",
        source(helper, "total += guard(dict, k) ? dict[k].size() : 0;")
    )
}

fn lower_tdz(src: &str) -> perry_hir::Module {
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    let mut checks = 0;
    perry_hir::tdz_check::for_each_module_expr(&hir, &mut |e| {
        checks += usize::from(perry_hir::tdz_check::is_read_check(e));
    });
    assert!(
        checks > 0,
        "the helper must actually contain a TDZ check: {hir:?}"
    );
    hir
}

fn tdz_verdict(hir: &perry_hir::Module, expected: bool) {
    verdict_facts(
        compute(&[("t.ts".to_string(), hir)], &FactsConfig::default()),
        expected,
    );
}

#[test]
fn helper_tdz_early_reference_site_proven() {
    tdz_verdict(&lower_tdz(&tdz_source(HELPER)), true);
}

#[test]
fn helper_tdz_exported_site_proven() {
    // An import makes the exported body reachable through an import cycle,
    // as in compiler/core.ts, even with the binding's declaration above it.
    let src = format!(
        "import {{ peer }} from './peer';\n{}",
        source(
            &format!("export {HELPER}"),
            "total += guard(dict, k) ? dict[k].size() : 0;"
        )
    );
    tdz_verdict(&lower_tdz(&src), true);
}

#[test]
fn helper_tdz_closure_site_proven() {
    let src = source("", "total += guard(dict, k) ? dict[k].size() : 0;");
    let src = format!("const guard = (map: any, key: any) => h.call(map, key);\n{src}");
    tdz_verdict(&lower_tdz(&src), true);
}

#[test]
fn helper_tdz_reassigned_binding_unproven() {
    let src = tdz_source(HELPER).replace("const h =", "let h: any =");
    let src = format!("{src}\nh = () => true;");
    tdz_verdict(&lower_tdz(&src), false);
}

#[test]
fn helper_tdz_mixed_binding_unproven() {
    let src = tdz_source(HELPER).replace(
        "const h = Object.prototype.hasOwnProperty;",
        "const h = Math.random() > 0.5 ? Object.prototype.hasOwnProperty : () => true;",
    );
    tdz_verdict(&lower_tdz(&src), false);
}

#[test]
fn helper_tdz_different_binding_unproven() {
    use perry_hir::{Expr, Stmt};
    let mut hir = lower_tdz(&tdz_source(HELPER));
    let f = hir
        .functions
        .iter_mut()
        .find(|f| f.name == "guard")
        .unwrap();
    let [Stmt::Return(Some(Expr::Sequence(parts)))] = f.body.as_mut_slice() else {
        panic!("expected the lowering's lifted return check: {:?}", f.body);
    };
    let [Expr::Call { args, .. }, Expr::Call { callee, .. }] = parts.as_mut_slice() else {
        panic!("expected check and call");
    };
    let Expr::PropertyGet { object, .. } = callee.as_ref() else {
        panic!("expected .call");
    };
    let Expr::LocalGet(h) = object.as_ref() else {
        panic!("expected binding");
    };
    assert!(matches!(&args[0], Expr::LocalGet(id) if id == h));
    // Keep the diagnostic label identical: only the slot's identity differs.
    args[0] = Expr::LocalGet(f.params[0].id);
    assert_ne!(f.params[0].id, *h);
    tdz_verdict(&hir, false);
}

#[test]
fn helper_tdz_conditional_unproven() {
    let src = tdz_source(
        "function guard(map: any, key: any) { return total > 0 ? h.call(map, key) : true; }",
    );
    tdz_verdict(&lower_tdz(&src), false);
}

#[test]
fn helper_tdz_side_effect_comma_unproven() {
    let src =
        tdz_source("function guard(map: any, key: any) { return (other(), h.call(map, key)); }");
    tdz_verdict(&lower_tdz(&src), false);
}
