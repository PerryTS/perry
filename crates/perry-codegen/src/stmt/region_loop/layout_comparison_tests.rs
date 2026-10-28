//! Getter, call and comparison inputs from the removed numeric-expression regions.
use perry_hir::{types::Type, CatchClause, Function, Module, Param};
use perry_hir::{CompareOp, Expr, Stmt};

fn get(object: Expr, key: &str) -> Expr {
    Expr::PropertyGet {
        byte_end: 0,
        object: Box::new(object),
        property: key.into(),
        byte_offset: 0,
    }
}

fn compare(op: CompareOp, left: Expr, right: Expr) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn ir(body: Vec<Stmt>, in_loop: bool) -> String {
    let param = |id| Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let protected = Stmt::Try {
        body,
        catch: Some(CatchClause {
            param: None,
            body: Vec::new(),
        }),
        finally: None,
    };
    let body = if in_loop {
        vec![Stmt::While {
            condition: Expr::Bool(true),
            body: vec![protected, Stmt::Break],
        }]
    } else {
        vec![protected]
    };
    let mut module = Module::new("bounded_numeric");
    module.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: Vec::new(),
        params: vec![param(1), param(2)],
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let mut opts = crate::temp_root_coverage::entry_opts();
    opts.is_entry_module = false;
    String::from_utf8(crate::compile_module(&module, opts).expect("bounded expression compiles"))
        .unwrap()
}

fn function(ir: &str) -> &str {
    let start = ir
        .find("define double @perry_fn_bounded_numeric__probe")
        .expect("probe body");
    let body = &ir[start..];
    &body[..body.find("\n}").expect("end of probe")]
}

fn check_comparison(output: &str) {
    crate::testing::verify_ir(output, "layout_comparisons").expect("valid semantic comparisons");
    assert!(
        !output.contains("rexpr.fast"),
        "runtime expression region was deleted"
    );
    assert!(
        function(output).contains("landingpad"),
        "getter/coercion throws retain the handler"
    );
}
#[test]
fn relational_property_inputs_preserve_operand_orientation_and_handler() {
    for op in [CompareOp::Lt, CompareOp::Le, CompareOp::Gt, CompareOp::Ge] {
        for read_left in [true, false] {
            let field = get(Expr::LocalGet(1), "value");
            let expr = if read_left {
                compare(op, field, Expr::Integer(166))
            } else {
                compare(op, Expr::LocalGet(2), field)
            };
            let output = ir(vec![Stmt::Expr(expr)], true);
            check_comparison(&output);
            assert_eq!(
                function(&output).matches("landingpad ").count(),
                1,
                "one handler tree per comparison"
            );
        }
    }
}
#[test]
fn left_getter_chain_is_evaluated_once_before_right_getter() {
    crate::temp_root_coverage::under_both_lowerings(|_| {
        let output = ir(
            vec![Stmt::Expr(compare(
                CompareOp::Lt,
                get(get(Expr::LocalGet(2), "data"), "length"),
                get(Expr::LocalGet(1), "value"),
            ))],
            true,
        );
        check_comparison(&output);
        let body = function(&output);
        let targets: std::collections::HashSet<_> = body
            .lines()
            .filter_map(|l| l.split("unwind label %").nth(1))
            .collect();
        assert_eq!(targets.len(), 1, "all operand reads share the active pad");
        assert_eq!(
            body.lines()
                .filter(|l| {
                    l.contains("@js_object_get_field_slow(")
                        || l.contains("@js_object_get_field_value(")
                        || l.contains("@js_object_get_field_value_site(")
                })
                .count(),
            3,
            "data, length and right value have one GetValue lowering each"
        );
    });
}
#[test]
fn an_observable_call_before_a_field_comparison_has_no_reused_region() {
    let output = ir(
        vec![
            Stmt::Expr(Expr::Call {
                byte_end: 0,
                callee: Box::new(Expr::LocalGet(2)),
                args: vec![],
                type_args: vec![],
                byte_offset: 0,
            }),
            Stmt::Expr(compare(
                CompareOp::Lt,
                get(Expr::LocalGet(1), "value"),
                Expr::Integer(166),
            )),
        ],
        true,
    );
    check_comparison(&output);
    assert!(
        function(&output).contains("invoke "),
        "observable call retains unwind"
    );
}
#[test]
fn captured_mutable_receiver_keeps_semantic_field_reads() {
    let closure = Expr::Closure {
        func_id: 2,
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::LocalSet(1, Box::new(Expr::Undefined)))],
        captures: vec![1],
        mutable_captures: vec![1],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    };
    let output = ir(
        vec![
            Stmt::Expr(closure),
            Stmt::Expr(compare(
                CompareOp::Ge,
                get(Expr::LocalGet(1), "value"),
                Expr::Integer(166),
            )),
        ],
        true,
    );
    check_comparison(&output);
    assert!(
        function(&output).contains("object_get_field"),
        "receiver is reread semantically"
    );
}
#[test]
fn equality_nested_receivers_labels_and_straight_line_keep_getvalue() {
    for expr in [
        compare(
            CompareOp::Eq,
            get(Expr::LocalGet(1), "value"),
            Expr::Integer(1),
        ),
        compare(
            CompareOp::Lt,
            get(get(Expr::LocalGet(1), "nested"), "value"),
            Expr::Integer(1),
        ),
        compare(
            CompareOp::Lt,
            get(Expr::LocalGet(1), "value"),
            get(get(Expr::LocalGet(2), "nested"), "other"),
        ),
    ] {
        check_comparison(&ir(vec![Stmt::Expr(expr)], true));
    }
    let expr = compare(
        CompareOp::Ge,
        get(Expr::LocalGet(1), "value"),
        Expr::Integer(166),
    );
    check_comparison(&ir(vec![Stmt::Expr(expr.clone())], false));
    check_comparison(&ir(
        vec![Stmt::Labeled {
            label: "outer".into(),
            body: Box::new(Stmt::Expr(expr)),
        }],
        true,
    ));
}
#[test]
fn a_property_number_does_not_retype_the_next_local_comparison() {
    let output = ir(
        vec![
            Stmt::Expr(compare(
                CompareOp::Lt,
                Expr::LocalGet(2),
                get(Expr::LocalGet(1), "value"),
            )),
            Stmt::Expr(compare(CompareOp::Lt, Expr::LocalGet(2), Expr::LocalGet(1))),
        ],
        true,
    );
    check_comparison(&output);
    assert!(
        function(&output).contains("@js_rel_lt("),
        "unproven locals retain coercion"
    );
}
#[test]
fn nan_zero_and_infinities_keep_ordered_relational_semantics() {
    for n in [
        f64::NAN,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::from_bits(0xfffd_0000_0000_0001),
    ] {
        let output = ir(
            vec![Stmt::Expr(compare(
                CompareOp::Le,
                get(Expr::LocalGet(1), "value"),
                Expr::Number(n),
            ))],
            true,
        );
        check_comparison(&output);
        assert!(
            function(&output).contains("fcmp ole double")
                || function(&output).contains("@js_rel_le("),
            "ordered comparison/coercion remains"
        );
    }
}
