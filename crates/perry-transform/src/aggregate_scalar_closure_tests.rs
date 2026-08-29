use super::*;

fn property(object: Expr, name: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(object),
        property: name.to_string(),
        byte_offset: 0,
    }
}

fn assert_carrier_is_materialized(module: &Module) {
    assert!(module.init.iter().any(|stmt| {
        matches!(
            stmt,
            Stmt::Let {
                id: 1,
                init: Some(Expr::Array(_)),
                ..
            }
        )
    }));
}

#[test]
fn reference_from_generated_function_keeps_materialized_aggregate() {
    let mut module = tests::aggregate_fixture(false);
    module.functions.push(Function {
        id: 99,
        name: "__obj_method_computed".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(1)))],
        is_async: false,
        is_generator: false,
        is_strict: false,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });

    run(&mut module);

    assert_carrier_is_materialized(&module);
}

#[test]
fn module_local_reference_from_closure_body_keeps_materialized_aggregate() {
    let mut module = tests::aggregate_fixture(false);
    *module.init.last_mut().expect("observer statement") = Stmt::Expr(Expr::Closure {
        func_id: 99,
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(property(
            Expr::IndexGet {
                object: Box::new(Expr::LocalGet(1)),
                index: Box::new(Expr::Integer(0)),
            },
            "component",
        )))],
        captures: Vec::new(),
        mutable_captures: Vec::new(),
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: true,
        is_async: false,
        is_generator: false,
        is_strict: true,
    });

    run(&mut module);

    assert_carrier_is_materialized(&module);
}
