use super::*;

fn sites(src: &str) -> EnumImageSites {
    let module = perry_parser::parse_typescript(src, "enum-image.ts").expect("source parses");
    enum_image_sites(&module)
}

fn only_plan(src: &str) -> EnumImagePlan {
    let out = sites(src);
    assert_eq!(out.sites.len(), 1, "refused: {:?}", out.refused);
    out.sites.into_values().next().unwrap()
}

fn keys(plan: &EnumImagePlan) -> Vec<String> {
    plan.stores.iter().map(|(k, _)| k.clone()).collect()
}

#[test]
fn esbuild_form_admits_number_string_and_reverse_stores() {
    let plan = only_plan(
        r#"var K = ((K2) => { K2[K2["A"] = 0] = "A"; K2[K2["N"] = -1] = "N"; K2["S"] = "s"; return K2; })(K || {});"#,
    );
    assert!(!plan.assigns);
    assert_eq!(keys(&plan), ["A", "0", "N", "-1", "S"]);
    assert!(matches!(plan.stores[1].1, EnumValue::String(ref s) if s == "A"));
    assert!(matches!(plan.stores[2].1, EnumValue::Number(-1)));
}

#[test]
fn tsc_form_admits_with_the_parameter_reusing_the_name() {
    let plan = only_plan(
        r#"var E; (function (E) { E[E["A"] = 0] = "A"; E[E["B"] = 1] = "B"; })(E || (E = {}));"#,
    );
    assert!(plan.assigns);
    assert_eq!(keys(&plan), ["A", "0", "B", "1"]);
}

#[test]
fn a_second_write_refuses_merged_and_preassigned_objects() {
    for src in [
        // Merged declarations.
        r#"var M = ((M2) => { M2[M2["X"] = 0] = "X"; return M2; })(M || {});
           var M = ((M2) => { M2[M2["Y"] = 1] = "Y"; return M2; })(M || {});"#,
        // Assigned elsewhere (a function may run first).
        r#"function f() { P = {}; }
           var P = ((P2) => { P2[P2["X"] = 0] = "X"; return P2; })(P || {});"#,
        // A namespace merging into the enum's name.
        r#"var Q = ((Q2) => { Q2[Q2["X"] = 0] = "X"; return Q2; })(Q || {}); Q++;"#,
    ] {
        let out = sites(src);
        assert!(out.sites.is_empty(), "{src}");
        assert!(
            out.refused.get("not_fresh").copied().unwrap_or(0) >= 1,
            "{src}"
        );
    }
}

#[test]
fn loops_eval_and_non_constant_bodies_refuse() {
    let cases = [
        (
            r#"for (;;) { var L = ((L2) => { L2[L2["X"] = 0] = "X"; return L2; })(L || {}); }"#,
            "in_loop",
        ),
        (
            r#"eval("1"); var V = ((V2) => { V2[V2["X"] = 0] = "X"; return V2; })(V || {});"#,
            "eval_or_with",
        ),
        (
            r#"var C = ((C2) => { C2[C2["X"] = f()] = "X"; return C2; })(C || {});"#,
            "value",
        ),
        (
            r#"var H = ((H2) => { H2[H2["X"] = 0.5] = "X"; return H2; })(H || {});"#,
            "value",
        ),
        (
            r#"var P = ((P2) => { P2["__proto__"] = "x"; return P2; })(P || {});"#,
            "proto_key",
        ),
        (
            r#"var B = ((B2) => { B2[B2["X"] = 0] = "X"; log(B2); return B2; })(B || {});"#,
            "body",
        ),
    ];
    for (src, reason) in cases {
        let out = sites(src);
        assert!(out.sites.is_empty(), "{src}");
        assert_eq!(
            out.refused.get(reason).copied(),
            Some(1),
            "{src}: {:?}",
            out.refused
        );
    }
}

#[test]
fn a_function_body_enum_is_fresh_per_activation() {
    let out = sites(
        r#"for (const x of xs) { g(() => { var F = ((F2) => { F2[F2["X"] = 0] = "X"; return F2; })(F || {}); return F; }); }"#,
    );
    assert_eq!(out.sites.len(), 1, "{:?}", out.refused);
}

#[test]
fn writes_outside_the_function_cannot_reach_its_var() {
    // perry's CommonJS wrapper: the body runs in a factory, and the module
    // also binds the export's name outside it.
    let plan = only_plan(
        r#"function factory() {
             var K = ((K2) => { K2[K2["A"] = 0] = "A"; return K2; })(K || {});
             return K;
           }
           const { K } = factory();"#,
    );
    assert_eq!(keys(&plan), ["A", "0"]);
}

#[test]
fn let_and_an_outer_tsc_var_refuse() {
    // `let`: the argument reads `L` in its dead zone and throws.
    let out = sites(r#"let L = ((L2) => { L2[L2["X"] = 0] = "X"; return L2; })(L || {});"#);
    assert!(out.sites.is_empty());
    // tsc form whose `var E` belongs to an outer function: `E` may already
    // hold an object when the inner function runs.
    let out = sites(
        r#"function outer() {
             var E;
             function inner() { (function (E) { E[E["A"] = 0] = "A"; })(E || (E = {})); }
             inner(); inner();
           }"#,
    );
    assert!(out.sites.is_empty(), "{:?}", out.refused);
    assert_eq!(out.refused.get("not_fresh").copied(), Some(1));
}
