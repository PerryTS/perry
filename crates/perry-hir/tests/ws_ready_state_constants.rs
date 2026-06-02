use perry_diagnostics::SourceCache;
use perry_hir::{
    clear_current_module_source, fix_local_native_instances, lower_module, Expr, Stmt,
};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed =
        parse_typescript_with_cache(src, "/tmp/ws_ready_state.ts", &mut cache).expect("parse");
    let mut module = lower_module(&parsed.module, "test", "/tmp/ws_ready_state.ts").expect("lower");
    clear_current_module_source();
    fix_local_native_instances(&mut module);
    module
}

fn let_number(module: &perry_hir::Module, name: &str) -> f64 {
    module
        .init
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let {
                name: stmt_name,
                init: Some(Expr::Number(value)),
                ..
            } if stmt_name == name => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected numeric let binding `{name}` in {:#?}",
                module.init
            )
        })
}

#[test]
fn ws_ready_state_constants_lower_on_default_namespace_and_class_shapes() {
    let module = lower(
        r#"
        import WebSocket, { WebSocket as NamedWebSocket } from "ws";
        import * as ws from "ws";

        const fromDefault = WebSocket.OPEN;
        const fromNamespace = ws.CLOSING;
        const fromNamespaceClass = ws.WebSocket.CLOSED;
        const fromNamedClass = NamedWebSocket.CONNECTING;
        "#,
    );

    assert_eq!(let_number(&module, "fromDefault"), 1.0);
    assert_eq!(let_number(&module, "fromNamespace"), 2.0);
    assert_eq!(let_number(&module, "fromNamespaceClass"), 3.0);
    assert_eq!(let_number(&module, "fromNamedClass"), 0.0);
}
