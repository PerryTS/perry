use perry_diagnostics::SourceCache;
use perry_hir::{lower_module, Expr, Module, Stmt};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn lower_err(src: &str) -> String {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            // #5245: the unimplemented-API gate only refuses at compile time in
            // strict mode; the default now defers to a throw-on-reach value.
            perry_hir::set_unimplemented_strict_mode(true);
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            let err = lower_module(&parsed.module, "test", "test.ts")
                .expect_err("lowering should reject unsupported API")
                .to_string();
            perry_hir::set_unimplemented_strict_mode(false);
            err
        })
        .expect("spawn lower thread")
        .join()
        .expect("lower thread panicked")
}

fn find_native_method_call<'a>(expr: &'a Expr, method: &str) -> Option<(&'a str, Option<&'a str>)> {
    match expr {
        Expr::NativeMethodCall {
            module,
            class_name,
            object,
            method: call_method,
            args,
        } => {
            if call_method == method {
                return Some((module.as_str(), class_name.as_deref()));
            }
            object
                .as_deref()
                .and_then(|object| find_native_method_call(object, method))
                .or_else(|| {
                    args.iter()
                        .find_map(|arg| find_native_method_call(arg, method))
                })
        }
        _ => None,
    }
}

#[test]
fn util_types_namespace_call_lowers_to_direct_module_key() {
    let module = lower(
        r#"
        import * as util from "util";
        const result = util.types.isMap(new Map());
    "#,
    );

    let call = module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                init: Some(expr), ..
            }
            | Stmt::Expr(expr) => find_native_method_call(expr, "isMap"),
            _ => None,
        })
        .expect("util.types.isMap should lower to a NativeMethodCall");

    assert_eq!(call, ("util/types", None));
}

#[test]
fn util_types_namespace_call_uses_canonical_api_gate() {
    let err = lower_err(
        r#"
        import * as util from "node:util";
        const result = util.types.notReal();
    "#,
    );

    assert!(
        err.contains("`util.types.notReal` is not implemented in Perry"),
        "unexpected error: {err}"
    );
}
