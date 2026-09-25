//! Native system predicates return i64 flags, which must become JS booleans.
use perry_codegen::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Import, ImportSpecifier, Module, ModuleKind, Stmt};

fn dark_mode_ir(target: Option<&str>) -> String {
    let mut module = Module::new("system_boolean_result");
    module.imports.push(Import {
        source: "perry/system".into(),
        specifiers: vec![ImportSpecifier::Named {
            imported: "isDarkMode".into(),
            local: "isDarkMode".into(),
        }],
        resolved_path: Some("perry/system".into()),
        is_native: true,
        module_kind: ModuleKind::NativeRust,
        type_only: false,
        runtime_erased: false,
        is_dynamic: false,
        is_dynamic_target: false,
        is_deferred_require: false,
        is_adopted_require: false,
    });
    module.functions.push(Function {
        id: 0,
        name: "darkModeResult".into(),
        type_params: vec![],
        params: vec![],
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "isDarkMode".into(),
                param_types: vec![],
                return_type: Type::Any,
            }),
            args: vec![],
            type_args: vec![],
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: vec![],
        decorators: vec![],
        was_plain_async: false,
        was_unrolled: false,
    });
    let options = CompileOptions {
        emit_ir_only: true,
        target: target.map(str::to_string),
        ..Default::default()
    };
    String::from_utf8(compile_module(&module, options).unwrap()).unwrap()
}

#[test]
fn system_predicate_calls_use_integer_abi_and_box_boolean_results() {
    for target in [None, Some("aarch64-linux-android")] {
        let ir = dark_mode_ir(target);
        let path = std::env::temp_dir().join(format!(
            "perry-system-boolean-{}-{}.ll",
            std::process::id(),
            target.unwrap_or("host")
        ));
        std::fs::write(&path, &ir).unwrap();
        assert!(
            ir.contains("call i64 @perry_system_is_dark_mode()"),
            "wrong native ABI ({path:?}):\n{ir}"
        );
        assert!(!ir.contains("call double @perry_system_is_dark_mode()"));
        assert!(
            ir.contains("icmp ne i64"),
            "native flags must be tested against zero"
        );
        assert!(
            ir.contains("9222246136947933188") && ir.contains("9222246136947933187"),
            "result must use true/false tags, not numeric 1/0"
        );
    }
}
