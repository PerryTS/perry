//! Values consumed after an inline array birth must survive its slow arm.
use super::*;

fn assert_live_origin(body: &str, point_callee: &str, source_callee: &str, count: usize) {
    let points: Vec<_> = body
        .lines()
        .filter(|line| line.contains("@llvm.experimental.gc.statepoint."))
        .map(parse_statepoint)
        .filter(|point| point.callee == point_callee)
        .collect();
    assert!(
        !points.is_empty(),
        "missing collecting birth {point_callee}"
    );
    assert!(
        points.iter().any(|point| {
            point
                .live
                .iter()
                .filter(|value| {
                    super::inline_birth::comes_from_call(body, value, source_callee, 64)
                })
                .count()
                >= count
        }),
        "{count} {source_callee} values must be live across {point_callee}: {points:?}"
    );
}

#[test]
fn array_elements_and_reduce_callback_are_live_across_inline_births() {
    for target in NATIVE_TARGETS {
        let module = entry_module(
            "array_element_birth.ts",
            vec![Stmt::Expr(Expr::Array(vec![
                Expr::Object(Vec::new()),
                Expr::Object(Vec::new()),
            ]))],
        );
        let ir = native_ir(&module, target, true);
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&ir, target, "array_birth").unwrap();
        assert_live_origin(
            function_slice(&rewritten, "main"),
            "js_inline_arena_slow_alloc",
            "js_object_alloc",
            2,
        );

        let mut module = bare_module("reduce_initial_birth.ts");
        module.init.push(Stmt::Expr(Expr::ArrayReduceRight {
            array: Box::new(Expr::Array(vec![Expr::Integer(1)])),
            callback: Box::new(Expr::Closure {
                func_id: 7,
                params: Vec::new(),
                return_type: Type::Any,
                body: vec![Stmt::Return(Some(Expr::Undefined))],
                captures: Vec::new(),
                mutable_captures: Vec::new(),
                captures_this: false,
                captures_new_target: false,
                enclosing_class: None,
                is_arrow: true,
                is_async: false,
                is_generator: false,
                is_strict: true,
            }),
            initial: Some(Box::new(Expr::Array(Vec::new()))),
        }));
        let ir = native_ir(&module, target, true);
        let rewritten =
            crate::inprocess::statepoint_rewritten_ir(&ir, target, "reduce_birth").unwrap();
        assert_live_origin(
            function_slice(&rewritten, "main"),
            "perry_birth_empty_array",
            "js_closure_alloc",
            1,
        );
    }
}
