//! Imported function method spreads must preserve the function receiver.
use perry_hir::{types::Type, CallArg, Expr, Module, Stmt};

#[test]
fn imported_function_method_spreads_use_receiver_dispatch() {
    for method in ["call", "apply", "bind"] {
        let mut module = Module::new("imported_function_method_spread.ts");
        module.init = vec![Stmt::Expr(Expr::CallSpread {
            callee: Box::new(Expr::PropertyGet {
                object: Box::new(Expr::ExternFuncRef {
                    name: "fn".into(),
                    param_types: vec![Type::Any, Type::Any],
                    return_type: Type::Any,
                }),
                property: method.into(),
                byte_offset: 0,
            }),
            args: vec![CallArg::Spread(Expr::Array(vec![Expr::Null]))],
            type_args: vec![],
        })];
        let mut options = crate::CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        };
        options
            .import_function_prefixes
            .insert("fn".into(), "helper".into());
        options.imported_func_param_counts.insert("fn".into(), 2);
        let ir = String::from_utf8(crate::compile_module(&module, options).unwrap()).unwrap();
        assert!(
            ir.contains("call double @js_native_call_method_apply_by_id("),
            "{method} lost its imported function receiver:\n{ir}"
        );
        let lines: Vec<_> = ir.lines().collect();
        let dispatch = lines
            .iter()
            .position(|line| line.contains("call double @js_native_call_method_apply_by_id("))
            .unwrap();
        let receiver = lines[dispatch]
            .split_once("(double ")
            .unwrap()
            .1
            .split(',')
            .next()
            .unwrap();
        let receiver_load = lines
            .iter()
            .position(|line| line.trim_start().starts_with(&format!("{receiver} =")))
            .unwrap();
        let last_concat = lines
            .iter()
            .rposition(|line| {
                line.contains("@js_array_concat(") && !line.trim_start().starts_with("declare")
            })
            .unwrap();
        assert!(
            receiver_load > last_concat,
            "{method} must reload the receiver after argument allocation:\n{ir}"
        );
        assert!(
            !ir.contains("call double @js_closure_call_apply_with_spread("),
            "{method} must not call an unbound method value:\n{ir}"
        );
    }
}
