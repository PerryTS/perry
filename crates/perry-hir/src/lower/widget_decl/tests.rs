//! Tests for `widget_decl` lowering, extracted from `widget_decl.rs` to
//! keep it under the 2000-line cap (CI `check_file_size.sh`). Pure move.

use super::*;
use swc_common::DUMMY_SP;
use swc_ecma_ast as ast;

fn str_lit(value: &str) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Str(ast::Str {
        span: DUMMY_SP,
        value: value.into(),
        raw: None,
    }))
}

fn key_value(key: &str, value: ast::Expr) -> ast::PropOrSpread {
    ast::PropOrSpread::Prop(Box::new(ast::Prop::KeyValue(ast::KeyValueProp {
        key: ast::PropName::Ident(ast::IdentName {
            span: DUMMY_SP,
            sym: key.into(),
        }),
        value: Box::new(value),
    })))
}

fn array_lit(elems: Vec<ast::Expr>) -> ast::Expr {
    ast::Expr::Array(ast::ArrayLit {
        span: DUMMY_SP,
        elems: elems
            .into_iter()
            .map(|e| {
                Some(ast::ExprOrSpread {
                    spread: None,
                    expr: Box::new(e),
                })
            })
            .collect(),
    })
}

fn object_lit(props: Vec<ast::PropOrSpread>) -> ast::Expr {
    ast::Expr::Object(ast::ObjectLit {
        span: DUMMY_SP,
        props,
    })
}

#[test]
fn primitive_spec_recognized_scalars() {
    assert!(matches!(
        parse_entry_field_primitive_spec("number"),
        WidgetFieldType::Number
    ));
    assert!(matches!(
        parse_entry_field_primitive_spec("boolean"),
        WidgetFieldType::Boolean
    ));
    assert!(matches!(
        parse_entry_field_primitive_spec("string"),
        WidgetFieldType::String
    ));
}

#[test]
fn primitive_spec_unknown_falls_back_to_string() {
    assert!(matches!(
        parse_entry_field_primitive_spec("Date"),
        WidgetFieldType::String
    ));
    assert!(matches!(
        parse_entry_field_primitive_spec(""),
        WidgetFieldType::String
    ));
}

#[test]
fn primitive_spec_optional_and_array_suffixes() {
    assert!(matches!(
        parse_entry_field_primitive_spec("number?"),
        WidgetFieldType::Optional(inner) if matches!(*inner, WidgetFieldType::Number)
    ));
    assert!(matches!(
        parse_entry_field_primitive_spec("string[]"),
        WidgetFieldType::Array(inner) if matches!(*inner, WidgetFieldType::String)
    ));
    // `'boolean[]?'` — array suffix is parsed first, then optional.
    let nested = parse_entry_field_primitive_spec("boolean[]?");
    let WidgetFieldType::Optional(o) = nested else {
        panic!("expected Optional, got {:?}", nested);
    };
    let WidgetFieldType::Array(a) = *o else {
        panic!("expected Optional<Array>, got Optional<other>");
    };
    assert!(matches!(*a, WidgetFieldType::Boolean));
}

#[test]
fn entry_field_value_spec_object_literal_becomes_object() {
    // Inline `{ url: 'string', clicks: 'number' }`.
    let expr = object_lit(vec![
        key_value("url", str_lit("string")),
        key_value("clicks", str_lit("number")),
    ]);
    let ty = parse_entry_field_value_spec(&expr);
    let WidgetFieldType::Object(fields) = ty else {
        panic!("expected Object, got {:?}", ty);
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "url");
    assert!(matches!(fields[0].1, WidgetFieldType::String));
    assert_eq!(fields[1].0, "clicks");
    assert!(matches!(fields[1].1, WidgetFieldType::Number));
}

#[test]
fn entry_field_value_spec_array_of_objects_does_not_collapse_to_string() {
    // Inline `[{ url: 'string', clicks: 'number' }]`. This is the
    // exact shape that used to collapse to `String` in the old
    // parser, breaking SwiftUI `ForEach` over the field.
    let inner = object_lit(vec![
        key_value("url", str_lit("string")),
        key_value("clicks", str_lit("number")),
    ]);
    let expr = array_lit(vec![inner]);
    let ty = parse_entry_field_value_spec(&expr);
    let WidgetFieldType::Array(elem) = ty else {
        panic!("expected Array, got {:?}", ty);
    };
    let WidgetFieldType::Object(fields) = *elem else {
        panic!("expected Array<Object>, got Array<other>");
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].0, "url");
    assert_eq!(fields[1].0, "clicks");
}

fn ident_expr(name: &str) -> ast::Expr {
    ast::Expr::Ident(ast::Ident {
        span: DUMMY_SP,
        ctxt: Default::default(),
        sym: name.into(),
        optional: false,
    })
}

fn member_expr(obj: ast::Expr, prop: &str) -> ast::Expr {
    ast::Expr::Member(ast::MemberExpr {
        span: DUMMY_SP,
        obj: Box::new(obj),
        prop: ast::MemberProp::Ident(ast::IdentName {
            span: DUMMY_SP,
            sym: prop.into(),
        }),
    })
}

fn call_expr(callee: ast::Expr, args: Vec<ast::Expr>) -> ast::Expr {
    ast::Expr::Call(ast::CallExpr {
        span: DUMMY_SP,
        ctxt: Default::default(),
        callee: ast::Callee::Expr(Box::new(callee)),
        args: args
            .into_iter()
            .map(|e| ast::ExprOrSpread {
                spread: None,
                expr: Box::new(e),
            })
            .collect(),
        type_args: None,
    })
}

fn num_lit(n: f64) -> ast::Expr {
    ast::Expr::Lit(ast::Lit::Num(ast::Number {
        span: DUMMY_SP,
        value: n,
        raw: None,
    }))
}

#[test]
fn format_expr_recognizes_string_cast() {
    // `String(entry.totalClicks)`
    let expr = call_expr(
        ident_expr("String"),
        vec![member_expr(ident_expr("entry"), "totalClicks")],
    );
    let fmt = try_parse_widget_format_expr(&expr).expect("expected Formatted match");
    assert!(matches!(fmt.call, WidgetFormatCall::StringCast));
    assert!(matches!(
        fmt.arg,
        WidgetFormatArg::Field(ref f) if f == "totalClicks"
    ));
}

#[test]
fn format_expr_recognizes_math_round_floor_ceil() {
    for (method, expected) in [
        ("round", WidgetFormatCall::Round),
        ("floor", WidgetFormatCall::Floor),
        ("ceil", WidgetFormatCall::Ceil),
    ] {
        let callee = member_expr(ident_expr("Math"), method);
        let expr = call_expr(
            callee,
            vec![member_expr(ident_expr("entry"), "totalClicks")],
        );
        let fmt = try_parse_widget_format_expr(&expr).expect("expected Math.* to match whitelist");
        assert!(
            std::mem::discriminant(&fmt.call) == std::mem::discriminant(&expected),
            "method `{}` produced {:?}, want {:?}",
            method,
            fmt.call,
            expected
        );
    }
}

#[test]
fn format_expr_recognizes_to_fixed_with_digits_literal() {
    // `entry.totalClicks.toFixed(2)`
    let target = member_expr(ident_expr("entry"), "totalClicks");
    let callee = member_expr(target, "toFixed");
    let expr = call_expr(callee, vec![num_lit(2.0)]);
    let fmt = try_parse_widget_format_expr(&expr).expect("expected toFixed match");
    assert!(matches!(fmt.call, WidgetFormatCall::ToFixed { digits: 2 }));
    assert!(matches!(
        fmt.arg,
        WidgetFormatArg::Field(ref f) if f == "totalClicks"
    ));
}

#[test]
fn format_expr_rejects_unknown_call_shape() {
    // `fmt(entry.totalClicks)` — user-defined function, not in
    // the whitelist; must return None (caller falls back to an
    // empty literal rather than producing broken output).
    let expr = call_expr(
        ident_expr("fmt"),
        vec![member_expr(ident_expr("entry"), "totalClicks")],
    );
    assert!(try_parse_widget_format_expr(&expr).is_none());
}

#[test]
fn parse_text_content_call_path_collapses_for_unknown() {
    // Same as above but exercising the public entrypoint — verifies
    // that the call falls through to Literal("") rather than
    // silently dropping into Field("").
    let expr = call_expr(
        ident_expr("fmt"),
        vec![member_expr(ident_expr("entry"), "totalClicks")],
    );
    match parse_text_content(&expr) {
        WidgetTextContent::Literal(ref s) if s.is_empty() => {}
        other => panic!("expected Literal(\"\"), got {:?}", other),
    }
}

#[test]
fn parse_text_content_call_path_recognized_call_becomes_formatted() {
    // `Math.round(entry.totalClicks)` — the user's actual bug
    // shape. Must produce Formatted, not the catch-all empty
    // literal that lost the call body.
    let expr = call_expr(
        member_expr(ident_expr("Math"), "round"),
        vec![member_expr(ident_expr("entry"), "totalClicks")],
    );
    match parse_text_content(&expr) {
        WidgetTextContent::Formatted(fmt) => {
            assert!(matches!(fmt.call, WidgetFormatCall::Round));
            assert!(matches!(
                fmt.arg,
                WidgetFormatArg::Field(ref f) if f == "totalClicks"
            ));
        }
        other => panic!("expected Formatted, got {:?}", other),
    }
}

/// Parse `source`, extract the body statements of the first function
/// declaration, and run the reload-policy scanner over them.
fn scan_first_fn_body(source: &str) -> (Vec<u32>, bool) {
    let module = perry_parser::parse_typescript(source, "widget_reload_test.ts")
        .expect("test source parses");
    for item in &module.body {
        if let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Fn(fn_decl))) = item {
            let body = fn_decl.function.body.as_ref().expect("fn has body");
            let mut found = Vec::new();
            let mut unparsed = false;
            scan_provider_stmts_for_reload_policy(&body.stmts, &mut found, &mut unparsed);
            return (found, unparsed);
        }
    }
    panic!("no function declaration in test source");
}

#[test]
fn reload_policy_literal_minutes_becomes_seconds() {
    let (found, unparsed) = scan_first_fn_body(
        r#"
            async function provider() {
                return {
                    entries: [{ temperature: 20 }],
                    reloadPolicy: { after: { minutes: 30 } },
                };
            }
            "#,
    );
    assert_eq!(found, vec![1800]);
    assert!(!unparsed);
}

#[test]
fn reload_policy_found_inside_if_and_try() {
    // The docs' error-handling shape: a short retry interval on the
    // failure path, a longer one on the happy path.
    let (found, unparsed) = scan_first_fn_body(
        r#"
            async function provider() {
                try {
                    const res = await fetch("https://api.example.com/weather");
                    if (!res.ok) {
                        return {
                            entries: [{ temperature: 0 }],
                            reloadPolicy: { after: { minutes: 5 } },
                        };
                    }
                    return {
                        entries: [{ temperature: 21 }],
                        reloadPolicy: { after: { minutes: 15 } },
                    };
                } catch (e) {
                    return {
                        entries: [{ temperature: 0 }],
                        reloadPolicy: { after: { minutes: 1 } },
                    };
                }
            }
            "#,
    );
    assert_eq!(found, vec![300, 900, 60]);
    assert!(!unparsed);
}

#[test]
fn reload_policy_non_literal_flags_unparsed() {
    let (found, unparsed) = scan_first_fn_body(
        r#"
            async function provider() {
                const policy = { after: { minutes: 10 } };
                return { entries: [], reloadPolicy: policy };
            }
            "#,
    );
    assert!(found.is_empty());
    assert!(unparsed);
}

#[test]
fn reload_policy_missing_is_neither_found_nor_unparsed() {
    let (found, unparsed) = scan_first_fn_body(
        r#"
            async function provider() {
                return { entries: [{ temperature: 20 }] };
            }
            "#,
    );
    assert!(found.is_empty());
    assert!(!unparsed);
}

#[test]
fn reload_policy_arrow_expression_body_scanned() {
    // `provider: async () => ({ entries: [], reloadPolicy: ... })` —
    // exercise the expression-body scanner directly.
    let module = perry_parser::parse_typescript(
        r#"const p = async () => ({ entries: [], reloadPolicy: { after: { minutes: 45 } } });"#,
        "widget_reload_arrow_test.ts",
    )
    .expect("test source parses");
    let mut found = Vec::new();
    let mut unparsed = false;
    for item in &module.body {
        if let ast::ModuleItem::Stmt(ast::Stmt::Decl(ast::Decl::Var(var))) = item {
            let init = var.decls[0].init.as_ref().expect("var has init");
            let ast::Expr::Arrow(arrow) = init.as_ref() else {
                panic!("expected arrow init");
            };
            let ast::BlockStmtOrExpr::Expr(expr) = arrow.body.as_ref() else {
                panic!("expected expression body");
            };
            scan_provider_return_expr_for_reload_policy(expr, &mut found, &mut unparsed);
        }
    }
    assert_eq!(found, vec![2700]);
    assert!(!unparsed);
}

#[test]
fn reload_policy_seconds_parser_edge_cases() {
    // Fractional minutes round to the nearest second.
    let half_minute = object_lit(vec![key_value(
        "after",
        object_lit(vec![key_value("minutes", num_lit(0.5))]),
    )]);
    assert_eq!(parse_reload_policy_seconds(&half_minute), Some(30));

    // Zero and negative intervals are rejected.
    let zero = object_lit(vec![key_value(
        "after",
        object_lit(vec![key_value("minutes", num_lit(0.0))]),
    )]);
    assert_eq!(parse_reload_policy_seconds(&zero), None);
    let negative = object_lit(vec![key_value(
        "after",
        object_lit(vec![key_value("minutes", num_lit(-5.0))]),
    )]);
    assert_eq!(parse_reload_policy_seconds(&negative), None);

    // Missing `minutes`, wrong key, or non-object shapes are rejected.
    let empty_after = object_lit(vec![key_value("after", object_lit(vec![]))]);
    assert_eq!(parse_reload_policy_seconds(&empty_after), None);
    let wrong_key = object_lit(vec![key_value(
        "every",
        object_lit(vec![key_value("minutes", num_lit(10.0))]),
    )]);
    assert_eq!(parse_reload_policy_seconds(&wrong_key), None);
    assert_eq!(parse_reload_policy_seconds(&str_lit("hourly")), None);
}

#[test]
fn entry_field_value_spec_array_with_string_suffix_spec() {
    // `'string[]'` and `['string']` both resolve to `Array<String>`.
    let by_suffix = parse_entry_field_value_spec(&str_lit("string[]"));
    let by_literal = parse_entry_field_value_spec(&array_lit(vec![str_lit("string")]));
    assert!(matches!(
        by_suffix,
        WidgetFieldType::Array(inner) if matches!(*inner, WidgetFieldType::String)
    ));
    assert!(matches!(
        by_literal,
        WidgetFieldType::Array(inner) if matches!(*inner, WidgetFieldType::String)
    ));
}
