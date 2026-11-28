//! An unknown `perry/system` function fails the compile instead of lowering to
//! no call at all.

use crate::{compile_module, CompileOptions};
use perry_hir::{Expr, Module, Stmt};

fn compile_call(method: &str, args: Vec<Expr>) -> Result<String, String> {
    let mut module = Module::new("system_unknown_call_test.ts");
    module.init.push(Stmt::Expr(Expr::NativeMethodCall {
        byte_offset: 0,
        module: "perry/system".to_string(),
        class_name: None,
        object: None,
        method: method.to_string(),
        args,
    }));
    compile_module(
        &module,
        CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        },
    )
    .map(|ir| String::from_utf8(ir).expect("LLVM IR is UTF-8"))
    .map_err(|err| format!("{err:?}"))
}

#[test]
fn unknown_function_is_a_compile_error() {
    let err = compile_call("totallyBogusFn", vec![Expr::Integer(1)]).unwrap_err();
    assert!(
        err.contains("perry/system: 'totallyBogusFn' is not a known function (args: 1)"),
        "{err}"
    );
}

#[test]
fn retired_function_names_its_replacement() {
    let err = compile_call("requestLocation", vec![Expr::Undefined]).unwrap_err();
    assert!(
        err.contains(
            "perry/system: 'requestLocation' was removed; \
             use geolocationGetCurrent(onSuccess, onError)"
        ),
        "{err}"
    );
}

#[test]
fn known_functions_still_lower() {
    let ir = compile_call("isDarkMode", vec![]).unwrap();
    assert!(ir.contains("@perry_system_is_dark_mode("), "{ir}");
    // Resolved ahead of the dispatch table.
    compile_call("getAppVersion", vec![]).unwrap();
}
