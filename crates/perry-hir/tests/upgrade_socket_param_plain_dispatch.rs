// The socket an HTTP `'upgrade'` listener receives
// (`server.on('upgrade', (req, socket, head) => …)`) is an ordinary
// `net.Socket` (decision 77: an HTTP connection IS a net.Socket payload), or,
// for a server with an attached WebSocketServer, a ws client handle with its
// own dynamic method dispatch. The compiler cannot tell the two apart, so the
// parameter must carry NO native-instance class: `socket.on('data', …)` then
// reaches the Socket's own prototype method (its flowing readable side), not
// a class-filtered `ws` Client row that registers the listener in a ws id
// table nobody fires (N8: the upgraded socket never saw `data`).
//
// Sabotage: re-tag the parameter `("ws", "Client")` in the call pre-scans and
// both assertions below go red (`module: "ws"` NativeMethodCalls reappear).

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
fn upgrade_listener_socket_param_lowers_to_ordinary_method_calls() {
    let module = lower(
        r#"
        import { createServer } from "node:http";
        import type { Duplex } from "node:stream";

        const server = createServer((req: any, res: any) => {
          res.end("ok");
        });

        server.on("upgrade", (req: any, socket: Duplex, head: Buffer) => {
          socket.on("data", (_part: Buffer) => {});
          socket.write("READY");
        });

        function helper(socket: any) {
          socket.send("x");
        }
        server.on("upgrade", (req: any, wsId: any, _head: any) => {
          wsId.on("message", (_msg: any) => {});
          helper(wsId);
        });
        "#,
    );

    let dump = format!("{module:#?}");
    let ws_dispatches = dump.matches("module: \"ws\"").count();
    assert_eq!(
        ws_dispatches, 0,
        "an upgrade listener's socket must not be lowered as a ws Client \
         NativeMethodCall ({ws_dispatches} found). Lowered HIR:\n{dump}"
    );
    assert!(
        !dump.contains("\"Client\""),
        "no upgrade parameter may be registered as a ws Client. Lowered HIR:\n{dump}"
    );
}
