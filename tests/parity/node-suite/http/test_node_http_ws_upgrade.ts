// Phase 4 acceptance — `httpServer.on('upgrade', (req, wsId, head) => …)`
// fires when a WebSocket client connects. The TS-side handler
// receives a `wsId` registered as a `("ws", "Client")` native
// instance via the upgrade pre-scan in HIR, so `wsId.send(...)` /
// `wsId.on(...)` / `wsId.close()` dispatch through NATIVE_MODULE_TABLE
// entries with class_filter=Some("Client") to the dedicated
// `js_ws_send_client_i64` / `js_ws_on_client_i64` /
// `js_ws_close_client_i64` shims (handles arrive as raw i64 after
// `unbox_to_i64` strips the POINTER_TAG that upgrade dispatch wraps
// the wsId in).
//
// Test pattern: server boots, websocat connects, exchanges a
// message via the standard ws.on/ws.send shape.

import { createServer } from "node:http";

const port = 18881;

const server = createServer((req: any, res: any) => {
  // Plain HTTP — confirms server is up before websocat dials.
  res.statusCode = 200;
  res.setHeader("Content-Type", "text/plain");
  res.end("perry-http-server with ws upgrade ready");
});

server.on("upgrade", (req: any, wsId: any, _head: any) => {
  console.log("[ws upgrade] " + req.method + " " + req.url + " — connecting client");
  wsId.on("message", (msg: string) => {
    console.log("[ws upgrade] received: " + msg);
    wsId.send("echo:" + msg);
  });
  // Send greeting so the client sees something on connect.
  wsId.send("perry-hello");
});

console.log("[node:http upgrade test] starting on " + port);
server.listen(port);

/*
@covers
crates/perry-stdlib/src/ws.rs:
  - js_ws_connect
  - js_ws_connect_start
  - js_ws_handle_to_i64
  - js_ws_is_open
  - js_ws_message_count
  - js_ws_process_pending
  - js_ws_receive
  - js_ws_send_to_client
  - js_ws_server_close
  - js_ws_server_new
  - js_ws_wait_for_message
*/
