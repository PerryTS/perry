use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, LogicalOp, Module, Param, Stmt};

fn comparison(op: CompareOp, n: f64) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(Expr::LocalGet(1)),
        right: Box::new(Expr::Number(n)),
    }
}
fn chain(n: usize) -> Expr {
    (1..n).fold(comparison(CompareOp::Ge, 100.0), |left, i| Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(left),
        right: Box::new(comparison(CompareOp::Ge, 100.0 + i as f64)),
    })
}
fn ir(expr: Expr, extra: Vec<Stmt>) -> String {
    let mut module = Module::new("comparison_graph");
    let mut body = extra;
    body.push(Stmt::Return(Some(expr)));
    module.functions.push(Function {
        id: 1,
        name: "predicate".into(),
        type_params: vec![],
        params: vec![Param {
            id: 1,
            name: "x".into(),
            ty: Type::Any,
            default: None,
            decorators: vec![],
            is_rest: false,
            arguments_object: None,
        }],
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    module.init.push(Stmt::Expr(Expr::Call {
        callee: Box::new(Expr::FuncRef(1)),
        args: vec![Expr::Undefined],
        type_args: vec![],
        byte_offset: 0,
    }));
    let ir = String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                output_type: "executable".into(),
                disable_constfn_shapes: false,
                program_has_worker: false,
                program_has_thread_agents: false,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap();
    let start = ir
        .find("define double @perry_fn_comparison_graph__predicate(")
        .expect(&ir);
    let end = start + ir[start..].find("\n}").unwrap();
    ir[start..end].to_string()
}

#[test]
fn forty_dynamic_comparisons_classify_once_and_share_the_slow_call() {
    let ir = ir(chain(40), vec![]);
    assert_eq!(ir.matches("call double @js_rel_ge(").count(), 1, "{ir}");
    assert_eq!(ir.matches("sitofp i32").count(), 1, "{ir}");
    assert_eq!(ir.matches("lshr i64").count(), 1, "{ir}");
    assert!(ir.contains("phi i1"), "{ir}");
    assert!(
        !ir.contains("phi double") || ir.matches("phi double").count() <= 2,
        "{ir}"
    );
}

#[test]
fn reassignment_declines_hoisting() {
    let ir = ir(
        chain(4),
        vec![Stmt::Expr(Expr::LocalSet(
            1,
            Box::new(Expr::String("11".into())),
        ))],
    );
    assert!(!ir.contains("cmpgraph."), "{ir}");
    assert_eq!(ir.matches("call double @js_rel_ge(").count(), 4, "{ir}");
}

#[test]
fn test_context_logical_selection_does_not_merge_boxed_operands() {
    let ir = ir(
        Expr::Conditional {
            condition: Box::new(Expr::Logical {
                op: LogicalOp::And,
                left: Box::new(Expr::LocalGet(1)),
                right: Box::new(Expr::Bool(true)),
            }),
            then_expr: Box::new(Expr::Number(1.0)),
            else_expr: Box::new(Expr::Number(0.0)),
        },
        vec![],
    );
    assert!(ir.contains("test.merge"), "{ir}");
    assert!(!ir.contains("logical.merge"), "{ir}");
}

#[test]
fn strict_number_literal_equalities_need_no_coercion_or_slow_arm() {
    let ir = ir(
        Expr::Logical {
            op: LogicalOp::Or,
            left: Box::new(comparison(CompareOp::Eq, 10.0)),
            right: Box::new(comparison(CompareOp::Ne, 12.0)),
        },
        vec![],
    );
    assert_eq!(ir.matches("sitofp i32").count(), 1, "{ir}");
    assert!(!ir.contains("cmpgraph.coerce"), "{ir}");
    assert!(!ir.contains("@js_eq("), "{ir}");
}
