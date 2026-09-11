//! Named ESM builtin cells must be initialized after their module's installer.
use crate::{compile_module, CompileOptions};
use perry_hir::{types::Type, Expr, Function, Module, Stmt};

fn compile_lookup(module_name: &str, property: &str) -> String {
    let mut module = Module::new("named_import_install.ts");
    module.functions.push(Function {
        id: 0,
        name: "lookup".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::Call {
            callee: Box::new(Expr::ExternFuncRef {
                name: "js_native_module_named_esm_export_value".into(),
                param_types: vec![Type::String, Type::String],
                return_type: Type::Any,
            }),
            args: vec![
                Expr::String(module_name.into()),
                Expr::String(property.into()),
            ],
            type_args: Vec::new(),
            byte_offset: 0,
        }))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let bytes = compile_module(
        &module,
        CompileOptions {
            emit_ir_only: true,
            ..Default::default()
        },
    )
    .expect("compile named import cell lookup");
    let ir = String::from_utf8(bytes).expect("LLVM IR");
    let start = ir
        .find("define double @perry_fn_named_import_install_ts__lookup(")
        .expect("lookup function emitted");
    let tail = &ir[start..];
    let end = tail.find("\n}\n").expect("lookup function terminated");
    tail[..end + 3].to_string()
}

#[test]
fn named_builtin_cell_installs_only_its_module_before_lookup() {
    for (module, property, installer) in [
        ("async_hooks", "AsyncResource", "js_nm_install_async_hooks"),
        (
            "node:async_hooks",
            "AsyncLocalStorage",
            "js_nm_install_async_hooks",
        ),
        ("util", "inherits", "js_nm_install_util"),
    ] {
        let ir = compile_lookup(module, property);
        let install = ir
            .find(&format!("call void @{installer}("))
            .expect("module installer emitted");
        let lookup = ir
            .find("call double @js_native_module_named_esm_export_value(")
            .expect("export cell lookup emitted");
        assert!(
            install < lookup,
            "installer must precede cache population: {ir}"
        );
        assert!(
            !ir.contains("@js_nm_install_all("),
            "no blanket install: {ir}"
        );
        assert!(
            !ir.contains("call void @js_nm_install_fs("),
            "no unrelated module: {ir}"
        );
    }
}

#[test]
fn unknown_named_module_does_not_install_unrelated_buckets() {
    let ir = compile_lookup("unknown-module", "value");
    assert!(ir.contains("call double @js_native_module_named_esm_export_value("));
    assert!(!ir.contains("call void @js_nm_install_"));
}
