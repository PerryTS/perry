//! A contract publication must precede the unknown call: otherwise the
//! ordinary first escape already sinks the result and hides rule R.

fn check_result_escape(summarized: bool, field_publication: bool, contract: bool) {
    use crate::{gen::Gen, model::TokKind};
    let source = "function F() { return { payload: 1 }; } const api = { F };";
    let ast = perry_parser::parse_typescript(source, "owner.ts").unwrap();
    let hir = perry_hir::lower_module(&ast, "owner", "owner.ts").unwrap();
    let mut g = Gen::new(vec![("owner.ts", &hir)], false, false);
    g.m.max_work = 50_000_000;
    g.m.per_unit = contract;
    g.register_all();
    g.link_exports();
    g.walk_all();
    let f =
        g.m.funcs
            .iter()
            .position(|f| f.name.ends_with(":F"))
            .unwrap() as u32;
    let ft = g.m.funcs[f as usize].token;
    g.m.funcs[f as usize].no_summary = !summarized;
    let publication = g.m.reason("published to another unit");
    let field = g.m.reason("function of a published literal field");
    if contract {
        g.m.pub_reason = Some(publication);
        g.m.field_fn_reason = Some(field);
        g.m.contract_fns.insert(f);
        // Publication happens before the owner hands api to unknown code.
        g.m.escape(
            ft,
            if field_publication {
                field
            } else {
                publication
            },
        );
    }
    g.m.summarize_and_replay();
    g.m.solve();
    assert_eq!(g.m.summarized(f), summarized, "both solver paths must run");
    let payload = g.m.atom("payload");
    let result =
        g.m.tmpls
            .iter()
            .position(|t| t.birth_keys.contains(&payload))
            .unwrap() as u32;
    let escaped = |m: &crate::model::Model| m.tmpl_toks(result).iter().any(|&t| m.is_escaped(t));
    if contract {
        assert!(!escaped(&g.m), "publication alone describes the result");
        assert!(g.m.is_escaped(ft), "F is already published");
    }
    let holder = g.m.atom("F");
    let api =
        g.m.tmpls
            .iter()
            .position(|t| t.birth_keys.contains(&holder))
            .unwrap() as u32;
    let api = g.m.tok(TokKind::Obj(api, crate::model::NO_CTX));
    let unknown = g.m.reason("passed to a call of an unknown function value");
    // That code can call api.F(), retain the object, and add/delete fields.
    g.m.escape(api, unknown);
    g.m.solve();
    assert!(!g.m.aborted);
    assert!(escaped(&g.m), "unknown code retains F's result after its contract publication (summarized={summarized}, field={field_publication})");
}

#[test]
fn a_published_function_escaping_later_also_escapes_its_result() {
    for summarized in [false, true] {
        check_result_escape(summarized, false, true);
    }
}

#[test]
fn a_published_field_function_escaping_later_also_escapes_its_result() {
    for summarized in [false, true] {
        check_result_escape(summarized, true, true);
    }
}

#[test]
fn unknown_code_keeps_an_unpublished_functions_result_too() {
    for summarized in [false, true] {
        check_result_escape(summarized, false, false);
    }
}

#[test]
fn a_closed_publication_escapes_results_only_when_callers_are_unknown() {
    use crate::{gen::Gen, model::TokKind};
    for summarized in [false, true] {
        for known in [true, false] {
            let ast = perry_parser::parse_typescript(
                "function F() { return { payload: 1 }; }",
                "owner.ts",
            )
            .unwrap();
            let hir = perry_hir::lower_module(&ast, "owner", "owner.ts").unwrap();
            let mut g = Gen::new(vec![("owner.ts", &hir)], false, false);
            g.m.max_work = 50_000_000;
            g.m.per_unit = true;
            g.register_all();
            g.link_exports();
            g.walk_all();
            let f =
                g.m.funcs
                    .iter()
                    .position(|f| f.name.ends_with(":F"))
                    .unwrap() as u32;
            let ft = g.m.funcs[f as usize].token;
            g.m.funcs[f as usize].no_summary = !summarized;
            g.m.contract_fns.insert(f);
            let r =
                g.m.reason("published to another unit (closed: its calls are known)");
            g.m.closed_pub_reason = Some(r);
            // No argument contract means the caller was unknown/over the
            // bound: a label saying closed grants no result contract.
            if known {
                g.m.closed_params.insert(f, vec![]);
            }
            g.m.escape(ft, r);
            g.m.summarize_and_replay();
            g.m.solve();
            assert_eq!(g.m.summarized(f), summarized);
            let payload = g.m.atom("payload");
            let tm =
                g.m.tmpls
                    .iter()
                    .position(|t| t.birth_keys.contains(&payload))
                    .unwrap() as u32;
            let _ = g.m.tok(TokKind::Obj(tm, crate::model::NO_CTX));
            let escaped = g.m.tmpl_toks(tm).iter().any(|&t| g.m.is_escaped(t));
            assert!(!g.m.aborted);
            assert_eq!(escaped, !known, "known={known}, summarized={summarized}");
        }
    }
}
