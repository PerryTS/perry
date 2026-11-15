//! SNAPK2c: the `compare_literal` hook reads only its own compare's operand
//! values. 4afa54803f kept an address-keyed site list across functions: a
//! compare whose numeric-literal or `typeof` operand is lowered without
//! `lower_expr` left a half-filled entry behind, and a later module's
//! expression at the freed literal's address emitted the hook with the first
//! module's register (Supabase: precise_roots.rs panic in an unrelated closure).

use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Module, Stmt};

fn ir_of(m: &Module) -> String {
    String::from_utf8(
        crate::compile_module(
            m,
            crate::CompileOptions {
                emit_ir_only: true,
                is_entry_module: true,
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn local(id: u32, ty: Type, init: Expr) -> Stmt {
    Stmt::Let {
        id,
        name: format!("k2c_{id}"),
        ty,
        mutable: true,
        init: Some(init),
    }
}

fn cmp(op: CompareOp, left: Expr, right: Expr) -> Expr {
    Expr::Compare {
        op,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn hooks(ir: &str) -> usize {
    ir.matches("call void @js_snap_compare_literal(").count()
}

/// Module A: equality compares lowered by the numeric-literal and `typeof`
/// fast paths (no hook). Module B: no equality compare at all, built after A
/// is dropped so its nodes reuse A's freed addresses.
#[test]
fn compare_literal_hook_never_crosses_compares() {
    std::env::set_var("PERRY_SNAP_CENSUS", "1");
    for round in 0..8 {
        let mut a = Module::new("k2c_a");
        a.init.push(local(0, Type::Any, Expr::Undefined));
        a.init.push(local(1, Type::Any, Expr::Undefined));
        for i in 0..48u32 {
            let lit = cmp(CompareOp::Eq, Expr::LocalGet(0), Expr::Number(1.0));
            let ty = cmp(
                CompareOp::Eq,
                Expr::TypeOf(Box::new(Expr::LocalGet(1))),
                Expr::String("string".into()),
            );
            a.init.push(local(10 + 2 * i, Type::Any, lit));
            a.init.push(local(11 + 2 * i, Type::Any, ty));
        }
        let ir_a = ir_of(&a);
        assert_eq!(hooks(&ir_a), 0, "fast-path compares emit no hook: {ir_a}");
        drop(a);
        let mut b = Module::new("k2c_b");
        b.init.push(local(0, Type::Any, Expr::Undefined));
        b.init.push(local(1, Type::Any, Expr::Undefined));
        for i in 0..96u32 {
            let sum = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::LocalGet(0)),
                right: Box::new(Expr::LocalGet(1)),
            };
            b.init.push(local(10 + i, Type::Any, sum));
        }
        let ir_b = ir_of(&b);
        assert_eq!(
            hooks(&ir_b),
            0,
            "round {round}: B has no equality compare: {ir_b}"
        );
    }
    // The generic compare emits one hook over its own two operand values.
    let mut c = Module::new("k2c_c");
    c.init.push(local(0, Type::Any, Expr::Undefined));
    c.init.push(local(
        1,
        Type::Any,
        cmp(
            CompareOp::Ne,
            Expr::LocalGet(0),
            Expr::String("linux".into()),
        ),
    ));
    let ir_c = ir_of(&c);
    std::env::remove_var("PERRY_SNAP_CENSUS");
    assert_eq!(hooks(&ir_c), 1, "{ir_c}");
    let call = ir_c
        .lines()
        .find(|l| l.contains("call void @js_snap_compare_literal("))
        .unwrap();
    assert!(call.contains("(i32 1, double "), "op !== : {call}");
    assert!(
        call.trim_end().ends_with("i32 2)"),
        "right is the literal: {call}"
    );
}
