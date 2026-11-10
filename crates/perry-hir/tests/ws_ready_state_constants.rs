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

/// #6117 — the global `WebSocket` (Perry's native client) folds its
/// ready-state statics to numbers.
#[test]
fn global_websocket_ready_state_constants_fold() {
    let module = lower(
        r#"
        const fromGlobal = WebSocket.OPEN;
        const closing = WebSocket.CLOSING;
        "#,
    );

    assert_eq!(let_number(&module, "fromGlobal"), 1.0);
    assert_eq!(let_number(&module, "closing"), 2.0);
}

/// npm `ws` compiles from source: an imported `WebSocket` is the package's
/// own class, so neither its statics nor its instances may be rewired to the
/// native client (`__websocket__`). A surviving `PropertyGet { .. "OPEN" }`
/// is exactly what reads the package's `WebSocket.OPEN` at runtime.
#[test]
fn imported_ws_websocket_is_the_package_class_not_the_native_client() {
    let module = lower(
        r#"
        import { WebSocket } from "ws";

        const ws = new WebSocket("ws://127.0.0.1:9");

        async function main() {
            while (ws.readyState !== WebSocket.OPEN) {
                ws.send("x");
            }
        }
        "#,
    );

    let main = module
        .functions
        .iter()
        .find(|f| f.name.contains("main"))
        .expect("lowered main function");
    let debug = format!("{:?}{:?}", module.init, main.body);
    assert!(
        debug.contains("\"OPEN\""),
        "an imported WebSocket.OPEN must stay a property read: {debug}"
    );
    assert!(
        !debug.contains("__websocket__"),
        "an imported WebSocket must not dispatch to the native client: {debug}"
    );
}
