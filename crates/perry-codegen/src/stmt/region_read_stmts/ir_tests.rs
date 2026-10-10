use super::*;
use perry_hir::{Function, Module, Param};

fn read(id: u32, key: &str) -> Expr {
    Expr::PropertyGet {
        object: Box::new(Expr::LocalGet(id)),
        property: key.into(),
        byte_offset: 0,
    }
}
fn binary(op: BinaryOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn binding(id: u32, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("v{id}"),
        ty: Type::Any,
        mutable: true,
        init: Some(init),
    }
}
fn body() -> Vec<Stmt> {
    let span = |id| Expr::Logical {
        op: LogicalOp::Or,
        left: Box::new(read(id, "span")),
        right: Box::new(Expr::Integer(1)),
    };
    vec![
        binding(10, read(1, "y")),
        binding(
            11,
            binary(
                BinaryOp::Add,
                binary(BinaryOp::Sub, read(1, "y"), Expr::Integer(1)),
                span(1),
            ),
        ),
        binding(12, read(2, "y")),
        binding(
            13,
            binary(
                BinaryOp::Add,
                binary(BinaryOp::Sub, read(2, "y"), Expr::Integer(1)),
                span(2),
            ),
        ),
        binding(
            14,
            Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(Expr::Compare {
                    op: perry_hir::CompareOp::Gt,
                    left: Box::new(Expr::LocalGet(10)),
                    right: Box::new(Expr::LocalGet(13)),
                }),
            },
        ),
        Stmt::Return(Some(Expr::LocalGet(14))),
    ]
}
fn ir(body: Vec<Stmt>) -> String {
    let param = |id| Param {
        id,
        name: format!("p{id}"),
        ty: Type::Any,
        default: None,
        decorators: Vec::new(),
        is_rest: false,
        arguments_object: None,
    };
    let mut module = Module::new("multi_read");
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
    String::from_utf8(crate::compile_module(&module, opts).unwrap()).unwrap()
}
fn guards(ir: &str) -> usize {
    ir.matches("call i32 @js_region_holder_read(").count()
}
#[test]
fn multi_key_read_region_has_one_guard_per_receiver_and_numeric_fast_body() {
    crate::temp_root_coverage::under_both_lowerings(|_| {
        let output = ir(body());
        assert_eq!(guards(&output), 2, "one word/shape guard per receiver");
        assert!(!output.contains("call double @js_object_get_field_ic_front"));
        assert!(output.contains("@js_dyn_index_get"));
        let fast = output.split("region.stmt.fast").last().unwrap();
        assert!(output.contains("fsub double") && output.contains("fadd double"));
        assert!(output.contains("fcmp ogt double"));
        assert!(
            output.contains("fcmp one double"),
            "zero and NaN are false for ||"
        );
        assert!(
            output.contains("select i1"),
            "undefined converts to canonical NaN only for arithmetic"
        );
        assert!(!fast.split("\n\n").next().unwrap().contains("@js_dynamic"));
    });
}
#[test]
fn multi_key_read_region_negative_control_detects_disabled_emission() {
    let _suppressed = region_guard::Suppressed::enter();
    let output = ir(body());
    assert_eq!(
        guards(&output),
        0,
        "the positive guard assertion must reject this arm"
    );
    assert!(!output.contains("region.stmt.fast"));
    assert!(output.contains("@js_object_get_field_ic_front"));
}
#[test]
fn multi_key_read_region_calls_and_third_receiver_end_the_run() {
    let mut stmts = body();
    stmts.insert(2, binding(16, read(3, "x")));
    let run = scan(&stmts, |_, _, _| true).unwrap();
    assert_eq!(run.receivers.len(), 2);
    assert_eq!(run.len, 3, "the later second receiver is the third overall");
    let mut stmts = body();
    stmts.insert(
        2,
        Stmt::Expr(Expr::Call {
            callee: Box::new(Expr::LocalGet(7)),
            args: Vec::new(),
            type_args: Vec::new(),
            byte_offset: 0,
        }),
    );
    assert_eq!(scan(&stmts, |_, _, _| true).unwrap().len, 2);
}
