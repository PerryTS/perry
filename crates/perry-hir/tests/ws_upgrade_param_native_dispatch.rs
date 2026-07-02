// Regression: the `wsId` parameter of an HTTP `'upgrade'` handler
// (`server.on('upgrade', (req, wsId, head) => …)`) must keep the
// `("ws", "Client")` native-instance tag the upgrade pre-scan assigns it, so
// `wsId.send(...)` / `wsId.on(...)` inside the handler lower to Client-class
// `Expr::NativeMethodCall { module: "ws", … }`.
//
// The arrow's own `wsId` param binding calls `shadow_native_instance_if_present`
// to tombstone any STALE native tag leaked from an outer scope — but here the
// tag is the FRESH, intended one the pre-scan just registered. Without the
// one-shot protection (`protect_native_param` / `prescan_protected_native_params`)
// that binding wrongly erased it, and the method calls fell back to generic
// dynamic dispatch (the post-upgrade "dead channel": `.send` / `.on` no longer
// routed to the ws Client shims).
//
// The handler arrow lowers to an inline `Expr::Closure { body, .. }` nested as
// the second argument of the `http.on` call (closures here are NOT lifted into
// `module.functions`). perry-hir's public walker (`walk_expr_children`) skips
// closure bodies and there is no public statement walker, so we assert over the
// module's `Debug` rendering instead: only `Expr::NativeMethodCall` carries a
// `module: "<name>"` field, so each `module: "ws"` occurrence is a surviving
// Client-class dispatch. Without the fix the calls degrade to `Expr::Call` and
// no `module: "ws"` appears.

use perry_diagnostics::SourceCache;
use perry_hir::{clear_current_module_source, fix_local_native_instances, lower_module};
use perry_parser::parse_typescript_with_cache;

fn lower(src: &str) -> perry_hir::Module {
    let mut cache = SourceCache::new();
    let parsed =
        parse_typescript_with_cache(src, "/tmp/ws_upgrade_param.ts", &mut cache).expect("parse");
    let mut module =
        lower_module(&parsed.module, "test", "/tmp/ws_upgrade_param.ts").expect("lower");
    clear_current_module_source();
    fix_local_native_instances(&mut module);
    module
}

#[test]
fn upgrade_handler_wsid_keeps_client_class_dispatch_across_its_own_binding() {
    let module = lower(
        r#"
        import { createServer } from "node:http";

        const server = createServer((req: any, res: any) => {
          res.end("ok");
        });

        server.on("upgrade", (req: any, wsId: any, _head: any) => {
          wsId.send("perry-hello");
          wsId.on("message", (_msg: any) => {});
        });
        "#,
    );

    let dump = format!("{module:#?}");
    let ws_dispatches = dump.matches("module: \"ws\"").count();

    // Both `wsId.send` and `wsId.on` must survive as ws Client NativeMethodCalls.
    assert!(
        ws_dispatches >= 2,
        "expected wsId.send + wsId.on to lower as ws Client-class NativeMethodCalls, \
         but found {ws_dispatches} `module: \"ws\"` dispatch(es). Lowered HIR:\n{dump}"
    );
    // `method: "send"` is unique to the ws Client call (the only other native
    // call here is `http.on`), so its presence pins the discriminating case.
    assert!(
        dump.contains("method: \"send\""),
        "wsId.send must dispatch as a ws NativeMethodCall. Lowered HIR:\n{dump}"
    );
}
