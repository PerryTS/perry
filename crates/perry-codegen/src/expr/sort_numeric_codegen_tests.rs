//! Verify that the HIR effect certificate reaches the actual operation call.
use crate::{
    compile_module,
    temp_root_coverage::{entry_opts, module_with_init},
};
use perry_hir::{types::Type, BinaryOp, Expr, Param, Stmt};

fn comparator(effect: bool) -> Expr {
    let mut body = vec![Stmt::Return(Some(Expr::Binary {
        op: BinaryOp::Sub,
        left: Box::new(Expr::LocalGet(17)),
        right: Box::new(Expr::LocalGet(23)),
    }))];
    if effect {
        body.insert(
            0,
            Stmt::Expr(Expr::LocalSet(17, Box::new(Expr::Number(0.0)))),
        );
    }
    Expr::Closure {
        func_id: 99,
        params: [17, 23]
            .into_iter()
            .map(|id| Param {
                id,
                name: format!("p{id}"),
                ty: Type::Number,
                default: None,
                decorators: vec![],
                is_rest: false,
                arguments_object: None,
            })
            .collect(),
        return_type: Type::Number,
        body,
        captures: vec![],
        mutable_captures: vec![],
        captures_this: false,
        captures_new_target: false,
        enclosing_class: None,
        is_arrow: false,
        is_async: false,
        is_generator: false,
        is_strict: true,
    }
}

#[test]
fn numeric_sort_operation_selected_only_with_a_whole_body_effect_proof() {
    for effect in [false, true] {
        let module = module_with_init(
            "numeric_sort.ts",
            vec![Stmt::Expr(Expr::ArraySort {
                array: Box::new(Expr::Array(
                    vec![Expr::Number(3.0), Expr::Number(1.0), Expr::Number(2.0)],
                    None,
                )),
                comparator: Box::new(comparator(effect)),
            })],
        );
        let bytes = compile_module(&module, entry_opts()).unwrap();
        let ir = String::from_utf8(bytes).unwrap();
        let main = crate::testing::root_slots::function_slice(&ir, "main");
        assert_eq!(
            main.contains("@js_array_sort_numeric_order("),
            !effect,
            "{main}"
        );
        assert_eq!(main.contains("@js_array_m_sort1("), effect, "{main}");
        assert_eq!(
            main.contains("@js_validate_array_comparator("),
            !effect,
            "{main}"
        );
    }
}

#[test]
fn keyed_sort_certificate_selects_a_rooted_property_operation() {
    let mut cmp = comparator(false);
    if let Expr::Closure { body, .. } = &mut cmp {
        if let Stmt::Return(Some(Expr::Binary { left, right, .. })) = &mut body[0] {
            *left = Box::new(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(17)),
                property: "rank".into(),
            });
            *right = Box::new(Expr::PropertyGet {
                byte_end: 0,
                byte_offset: 0,
                object: Box::new(Expr::LocalGet(23)),
                property: "rank".into(),
            });
        }
    }
    let module = module_with_init(
        "key_sort.ts",
        vec![Stmt::Expr(Expr::ArraySort {
            array: Box::new(Expr::Array(vec![Expr::Number(1.0)], None)),
            comparator: Box::new(cmp),
        })],
    );
    let ir = String::from_utf8(compile_module(&module, entry_opts()).unwrap()).unwrap();
    let main = crate::testing::root_slots::function_slice(&ir, "main");
    assert!(main.contains("@js_array_sort_key_order("), "{main}");
    assert!(!main.contains("@js_array_sort_numeric_order("), "{main}");
}
