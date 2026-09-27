//! #11408: check emitted calls, including omitted optional arguments, rather
//! than relying only on the source-level signature audit.
use crate::{compile_module, CompileOptions};
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module, Stmt};

fn native_call(module_name: &str, class: Option<&str>, method: &str, receiver: bool) -> String {
    let mut module = Module::new("native_abi_test.ts");
    module.functions.push(Function {
        id: 0,
        name: "probe".into(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Expr(Expr::NativeMethodCall {
            module: module_name.into(),
            class_name: class.map(str::to_owned),
            object: receiver.then(|| Box::new(Expr::Number(1.0))),
            method: method.into(),
            args: Vec::new(),
        })],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    String::from_utf8(
        compile_module(
            &module,
            CompileOptions {
                emit_ir_only: true,
                ..Default::default()
            },
        )
        .expect("native ABI fixture compiles"),
    )
    .unwrap()
}

fn call_line<'a>(ir: &'a str, signature: &str) -> &'a str {
    ir.lines()
        .find(|line| line.contains(&format!("call {signature}(")))
        .unwrap_or_else(|| panic!("missing call {signature}:\n{ir}"))
}

#[test]
fn native_abi_predicates_use_integer_registers_and_box_booleans() {
    for (module, class, method, signature, comparison) in [
        (
            "cheerio",
            None,
            "hasClass",
            "i1 @js_cheerio_selection_has_class",
            "icmp ne i1",
        ),
        (
            "ws",
            Some("Server"),
            "emit",
            "i32 @js_ws_server_emit",
            "icmp ne i32",
        ),
    ] {
        let ir = native_call(module, class, method, true);
        call_line(&ir, signature);
        assert!(
            ir.contains(comparison),
            "predicate must test its integer result:\n{ir}"
        );
        assert!(
            ir.contains("select i1"),
            "predicate must select JS boolean tags:\n{ir}"
        );
    }
}

#[test]
fn native_abi_missing_arguments_are_padded_with_the_runtime_types() {
    for (module, class, method, receiver, signature, params) in [
        (
            "cheerio",
            None,
            "children",
            true,
            "i64 @js_cheerio_selection_children",
            vec!["i64", "i64"],
        ),
        (
            "dgram",
            Some("Socket"),
            "eventNames",
            true,
            "double @js_dgram_socket_event_names",
            vec!["i64", "i64"],
        ),
        (
            "module",
            None,
            "createRequire",
            false,
            "double @js_module_create_require_devirt",
            vec!["double"],
        ),
        (
            "module",
            None,
            "findPackageJSON",
            false,
            "double @js_module_find_package_json",
            vec!["double", "double", "double"],
        ),
        (
            "nodemailer",
            None,
            "createTransport",
            false,
            "double @js_nodemailer_create_transport",
            vec!["double"],
        ),
        (
            "process",
            None,
            "setgroups",
            false,
            "void @js_process_setgroups",
            vec!["double"],
        ),
        (
            "process",
            None,
            "initgroups",
            false,
            "void @js_process_initgroups",
            vec!["double", "double"],
        ),
        (
            "net",
            Some("Socket"),
            "setKeyCert",
            true,
            "double @js_tls_socket_set_key_cert",
            vec!["i64", "double"],
        ),
    ] {
        let ir = native_call(module, class, method, receiver);
        let call = call_line(&ir, signature);
        let actual: Vec<_> = call
            .split_once('(')
            .unwrap()
            .1
            .split_once(')')
            .unwrap()
            .0
            .split(',')
            .map(|arg| arg.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(actual, params, "{module}.{method}: {call}");
    }
}

#[test]
fn native_abi_agent_accessors_and_websocket_on_use_the_runtime_return_type() {
    for method in ["sockets", "freeSockets", "requests"] {
        let ir = native_call("http", Some("Agent"), method, true);
        let symbol = if method == "freeSockets" {
            "free_sockets"
        } else {
            method
        };
        call_line(&ir, &format!("double @js_http_agent_{symbol}"));
    }
    let ir = native_call("ws", None, "on", true);
    call_line(&ir, "i64 @js_ws_on");
}

#[test]
fn native_abi_dynamic_array_assignment_passes_its_own_strictness() {
    for strict in [false, true] {
        let ir = crate::temp_root_coverage::main_ir_for(
            "dynamic_array_abi",
            vec![
                Stmt::Let {
                    id: 1,
                    name: "array".into(),
                    ty: Type::Array(Box::new(Type::Number)),
                    mutable: false,
                    init: Some(Expr::Array(vec![Expr::Number(0.0)])),
                },
                Stmt::Let {
                    id: 2,
                    name: "key".into(),
                    ty: Type::Any,
                    mutable: false,
                    init: Some(Expr::PropertyGet {
                        object: Box::new(Expr::Object(vec![(
                            "key".into(),
                            Expr::String("0".into()),
                        )])),
                        property: "key".into(),
                        byte_offset: 0,
                    }),
                },
                Stmt::Expr(Expr::PutValueSet {
                    target: Box::new(Expr::LocalGet(1)),
                    receiver: Box::new(Expr::LocalGet(1)),
                    key: Box::new(Expr::LocalGet(2)),
                    value: Box::new(Expr::Number(42.0)),
                    strict,
                }),
            ],
        );
        let call = call_line(&ir, "i64 @js_typed_feedback_array_set_index_or_string");
        assert!(
            call.contains(&format!(", i32 {})", i32::from(strict))),
            "{call}"
        );
    }
}
