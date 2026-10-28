//! THR1: builtin methods and computed-key calls on receivers the compiler
//! cannot type take the O(1) proving entries, not the by-name tower.

use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Param, Stmt};

fn param(id: u32, name: &str) -> Param {
    Param {
        id,
        name: name.into(),
        ty: Type::Any,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

/// `function f(a, k, x) { return <body>; }` with untyped parameters.
fn function_ir(body: Expr) -> String {
    let mut m = Module::new("untyped_dispatch");
    m.functions.push(Function {
        id: 1,
        name: "f".into(),
        type_params: vec![],
        params: vec![param(1, "a"), param(2, "k"), param(3, "x")],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(body))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    compile(&m)
}

fn compile(m: &Module) -> String {
    String::from_utf8(
        crate::compile_module(
            m,
            crate::CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .unwrap(),
    )
    .unwrap()
}

fn method_call(property: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1)),
            property: property.into(),
            byte_offset: 0,
        }),
        args,
        type_args: vec![],
        byte_offset: 0,
    }
}

/// A call of `symbol` (every entry is declared whether or not it is used).
fn calls(ir: &str, symbol: &str) -> bool {
    let callee = format!("@{symbol}(");
    ir.lines()
        .any(|l| l.contains(&callee) && (l.contains(" call ") || l.contains(" invoke ")))
}

#[test]
fn a_string_named_array_builtin_on_an_untyped_receiver_takes_the_builtin_entry() {
    for (property, entry) in [
        ("indexOf", "js_bm_indexOf1"),
        ("includes", "js_bm_includes1"),
        ("lastIndexOf", "js_bm_lastIndexOf1"),
    ] {
        let ir = function_ir(method_call(property, vec![Expr::LocalGet(3)]));
        assert!(
            ir.contains("anystr.generic"),
            "{property}: the String guard is kept"
        );
        assert!(
            calls(&ir, entry),
            "{property}: the non-string arm calls {entry}"
        );
        assert!(
            !calls(&ir, "js_typed_feedback_native_call_method_by_id"),
            "{property}: the non-string arm still reaches the by-name tower"
        );
    }
}

#[test]
fn a_string_only_builtin_keeps_the_dispatcher_on_the_non_string_arm() {
    let ir = function_ir(method_call("charAt", vec![Expr::LocalGet(3)]));
    assert!(!calls(&ir, "js_bm_charAt1"));
}

#[test]
fn an_order_free_computed_key_call_probes_the_key_before_the_dispatcher() {
    let ir = function_ir(Expr::Call {
        byte_end: 0,
        callee: Box::new(Expr::IndexGet {
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
        }),
        args: vec![Expr::LocalGet(3)],
        type_args: vec![],
        byte_offset: 0,
    });
    assert!(
        calls(&ir, "perry_probe_get_keyed"),
        "keyed call has no probe"
    );
    assert!(
        calls(&ir, "js_closure_call1"),
        "probe hit is not a direct closure call"
    );
    assert!(
        calls(&ir, "js_native_call_method_value"),
        "probe miss lost its dispatcher"
    );
}
