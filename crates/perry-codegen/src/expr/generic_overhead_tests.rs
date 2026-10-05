//! Pin both the removed generic work and the fallback that remains necessary.

use crate::testing::root_slots::function_slice;
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{CompareOp, Expr, Function, Module, Param, Stmt};

fn param(id: u32, ty: Type) -> Param {
    Param {
        id,
        name: format!("p{id}"),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn probe_ir(params: Vec<Param>, body: Vec<Stmt>) -> String {
    let mut module = Module::new("cost_test");
    module.init.push(Stmt::Let {
        id: 99,
        name: "sink".into(),
        ty: Type::Any,
        mutable: true,
        init: Some(Expr::Number(0.0)),
    });
    module.functions.push(Function {
        id: 1,
        name: "probe".into(),
        type_params: vec![],
        params,
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
    let ir = String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                is_entry_module: false,
                ..CompileOptions::default()
            },
        )
        .expect("compile cost probe"),
    )
    .unwrap();
    let mut body = function_slice(&ir, "perry_fn_cost_test__probe").to_string();
    // A declared numeric parameter can introduce a guarded ABI wrapper. The
    // erased-type fallback, not the wrapper's dispatch, owns its root store.
    let generic = "perry_fn_cost_test__probe$generic";
    if ir.contains(&format!("@{generic}(")) {
        body.push_str(function_slice(&ir, generic));
    }
    body
}

fn global_store_ir(value: Expr, ty: Type) -> String {
    probe_ir(
        vec![param(1, ty)],
        vec![
            Stmt::Expr(Expr::LocalSet(99, Box::new(value))),
            Stmt::Return(Some(Expr::LocalGet(99))),
        ],
    )
}

#[test]
fn scalar_global_stores_keep_the_store_without_root_shading() {
    for value in [
        Expr::Number(3.0),
        Expr::Number(-0.0),
        Expr::Number(f64::NAN),
        Expr::Integer(7),
        Expr::Bool(true),
        Expr::Null,
        Expr::Undefined,
        Expr::Compare {
            op: CompareOp::Eq,
            left: Box::new(Expr::LocalGet(1)),
            right: Box::new(Expr::Null),
        },
    ] {
        let ir = global_store_ir(value, Type::Any);
        assert!(
            ir.lines().any(|line| line.contains("store double")
                && line.contains("@perry_global_cost_test__99")),
            "store disappeared:\n{ir}"
        );
        assert!(
            !ir.contains("call void @js_write_barrier_root_nanbox("),
            "scalar barrier:\n{ir}"
        );
    }
}

#[test]
fn unknown_and_declared_number_globals_keep_root_shading() {
    for ty in [Type::Any, Type::Number] {
        let ir = global_store_ir(Expr::LocalGet(1), ty);
        assert!(
            ir.contains("call void @js_write_barrier_root_nanbox("),
            "annotation must not suppress the barrier:\n{ir}"
        );
    }
    for value in [
        Expr::String("a heap string longer than SSO".into()),
        Expr::Array(vec![Expr::Number(3.0)]),
        Expr::BigInt("123".into()),
    ] {
        let ir = global_store_ir(value, Type::Any);
        assert!(
            ir.contains("call void @js_write_barrier_root_nanbox("),
            "heap barrier missing:\n{ir}"
        );
    }
}
