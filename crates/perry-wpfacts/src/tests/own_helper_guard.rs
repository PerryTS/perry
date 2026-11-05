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
    let pf = facts(src);
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
