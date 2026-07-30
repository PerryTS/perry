use perry_diagnostics::SourceCache;
use perry_hir::{
    clear_current_module_source, fix_local_native_instances, lower_module, Expr, Stmt,
};
use perry_parser::parse_typescript_with_cache;

#[test]
fn local_perry_ui_state_value_uses_native_getter() {
    let mut cache = SourceCache::new();
    let parsed = parse_typescript_with_cache(
        r#"
        import { State } from "perry/ui";

        function main() {
            const text = State("");
            return text.value;
        }
        "#,
        "/tmp/state_value_native_dispatch.ts",
        &mut cache,
    )
    .expect("parse");
    let mut module = lower_module(
        &parsed.module,
        "test",
        "/tmp/state_value_native_dispatch.ts",
    )
    .expect("lower");
    clear_current_module_source();
    fix_local_native_instances(&mut module);

    let main = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("main function");
    let value = main.body.iter().find_map(|stmt| match stmt {
        Stmt::Return(Some(expr)) => Some(expr),
        _ => None,
    });

    assert!(
        matches!(
            value,
            Some(Expr::NativeMethodCall {
                module,
                class_name: None,
                object: Some(_),
                method,
                args,
            }) if module == "perry/ui" && method == "value" && args.is_empty()
        ),
        "State.value must lower through perry_ui_state_get, got: {value:#?}"
    );
}
