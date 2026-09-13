use super::*;
use crate::lower::{lower_expr, LoweringContext};

fn expression(source: &str) -> Box<ast::Expr> {
    let module = perry_parser::parse_typescript(&format!("({source});"), "test.ts").unwrap();
    let ast::ModuleItem::Stmt(ast::Stmt::Expr(stmt)) = &module.body[0] else {
        panic!()
    };
    let ast::Expr::Paren(paren) = stmt.expr.as_ref() else {
        panic!()
    };
    paren.expr.clone()
}

fn lower(source: &str) -> Expr {
    lower_expr(&mut LoweringContext::new("test.ts"), &expression(source)).unwrap()
}

#[test]
fn node_threshold_is_inclusive_and_small_arrays_stay_direct() {
    // The root array counts as one value node.
    let below = format!("[{}]", vec!["0"; MIN_NODES - 2].join(","));
    let at = format!("[{}]", vec!["0"; MIN_NODES - 1].join(","));
    assert!(matches!(lower(&below), Expr::Array(_)));
    let Expr::JsonParse(text) = lower(&at) else {
        panic!("threshold must select JSON.parse")
    };
    let Expr::String(text) = *text else { panic!() };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value.as_array().unwrap().len(), MIN_NODES - 1);
}

#[test]
fn primitive_array_text_threshold_is_inclusive() {
    let below = format!(r#"["{}"]"#, "x".repeat(MIN_TEXT_BYTES - 1));
    let at = format!(r#"["{}"]"#, "x".repeat(MIN_TEXT_BYTES));
    assert!(matches!(lower(&below), Expr::Array(_)));
    assert!(matches!(lower(&at), Expr::JsonParse(_)));
}

#[test]
fn record_text_threshold_counts_key_and_string_bytes_without_source_spans() {
    let below = format!(r#"{{key:"{}"}}"#, "x".repeat(CODEGEN_TEXT_BYTES - 4));
    let at = format!(r#"{{key:"{}"}}"#, "x".repeat(CODEGEN_TEXT_BYTES - 3));
    assert!(lower_large_json_literal(&expression(&below)).is_none());
    assert!(matches!(lower(&at), Expr::JsonParse(_)));
    assert!(!matches!(
        lower(r#"{model:"small",items:[1,true,null]}"#),
        Expr::JsonParse(_)
    ));
}

#[test]
fn non_primitive_array_node_threshold_is_inclusive() {
    // Root array and empty record count as two nodes; this is not a primitive array.
    let below = format!("[{{}},{}]", vec!["0"; CODEGEN_NODES - 3].join(","));
    let at = format!("[{{}},{}]", vec!["0"; CODEGEN_NODES - 2].join(","));
    assert!(lower_large_json_literal(&expression(&below)).is_none());
    assert!(matches!(lower(&at), Expr::JsonParse(_)));
}

#[test]
fn mid_size_typed_records_keep_static_shapes() {
    let records: Vec<_> = (0..400)
        .map(|i| format!(r#"{{id:{i},name:"n{i}",tags:["a","b"],w:0.25}}"#))
        .collect();
    let source = format!(
        "type Rec = {{id:number;name:string;tags:string[];w:number}}; const recs: Rec[] = [{}];",
        records.join(",")
    );
    let ast = perry_parser::parse_typescript(&source, "test.ts").unwrap();
    let hir = crate::lower::lower_module(&ast, "test", "test.ts").unwrap();
    assert!(!format!("{hir:?}").contains("JsonParse("));
    assert!(
        !hir.classes.is_empty(),
        "records must retain their static shapes"
    );
    let object = format!(r#"{{padding:"{}"}}"#, "x".repeat(MIN_TEXT_BYTES));
    assert!(!matches!(lower(&object), Expr::JsonParse(_)));
}

#[test]
fn only_flat_primitive_arrays_get_the_lower_threshold() {
    assert!(is_primitive_array(&expression(
        r#"[1,-0,+2,(3),"x",true,false,null]"#
    )));
    for source in [
        "[{}]",
        "[[1]]",
        "[undefined]",
        "[1,,2]",
        "[...other]",
        "[f()]",
        "{x:1}",
    ] {
        assert!(!is_primitive_array(&expression(source)), "{source}");
    }
    // A record/nested array after the low node threshold must still select
    // the higher tier; a short-circuiting size probe alone would miss it.
    for last in ["{}", "[]"] {
        let source = format!("[{},{}]", vec!["0"; MIN_NODES].join(","), last);
        assert!(lower_large_json_literal(&expression(&source)).is_none());
    }
}

#[test]
fn serialization_preserves_order_duplicates_escaping_and_numbers() {
    let expr = expression(r#"{z:0, a:-0, z:+2, 1e-7:0xff, nested:[true,null,"quote\"\n\\☃"]}"#);
    let mut text = Vec::new();
    serialize(&expr, &mut text, 0).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(text.starts_with(r#"{"z":0.0,"a":-0.0,"z":2.0,"1e-7":255.0,"nested":["#));
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["z"], 2.0);
    assert!(value["a"].as_f64().unwrap().is_sign_negative());
    assert_eq!(value["nested"][2], "quote\"\n\\☃");
}

#[test]
fn non_json_semantics_fall_back_even_after_the_size_probe_succeeds() {
    for value in [
        "{__proto__:null}",
        "{\"__proto__\":{}}",
        "[1,,2]",
        "[...other]",
        "{get x(){return 1}}",
        "{x:sideEffect()}",
        "{[key]:1}",
        "{x:undefined}",
        "{x:1e999}",
        "{x:/a/}",
        "{x:1n}",
        r#"{x:"\ud800"}"#,
    ] {
        // The probe visits the final array element first, reaching the size
        // threshold before serialization discovers the incompatible value.
        let source = format!(r#"[{value},"{}"]"#, "x".repeat(CODEGEN_TEXT_BYTES));
        let expr = expression(&source);
        assert!(is_large(&expr));
        assert!(lower_large_json_literal(&expr).is_none(), "{value}");
    }
}

#[test]
fn speculative_walk_is_bounded_for_deep_literals() {
    let source = format!(
        "{}0{}",
        "[".repeat(MAX_DEPTH + 2),
        "]".repeat(MAX_DEPTH + 2)
    );
    assert!(lower_large_json_literal(&expression(&source)).is_none());
}

#[test]
fn defines_fold_typeof_before_large_literal_lowering() {
    use std::collections::BTreeMap;
    let source = r#"declare const BIG: any; export const snapshot = typeof BIG === "undefined" ? undefined : BIG;"#;
    let module = perry_parser::parse_typescript(source, "test.ts").unwrap();
    for (value, expected_parse) in [
        (format!("[{}]", vec!["0"; MIN_NODES].join(",")), true),
        ("undefined".into(), false),
    ] {
        let defines =
            perry_parser::defines::Defines::parse(&BTreeMap::from([("BIG".into(), value)]))
                .unwrap();
        let ast = defines.apply(&module).unwrap();
        let hir = crate::lower::lower_module(&ast, "test", "test.ts").unwrap();
        let dump = format!("{hir:?}");
        assert_eq!(dump.contains("JsonParse("), expected_parse);
        assert!(!dump.contains("TypeOf("));
        assert!(!dump.contains("Conditional"));
    }
}
