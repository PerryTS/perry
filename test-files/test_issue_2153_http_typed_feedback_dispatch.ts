// Refs #2153: when an HttpServer handle reaches the runtime through an
// `any`-typed receiver, the codegen emits
// `js_typed_feedback_native_call_method` → `js_native_call_method`. Pre-fix,
// the dispatcher had no `HttpServer` arm — `server.listen / .close / .on /
// .address` all resolved to undefined-or-NaN even though the corresponding
// `class_filter: Some("HttpServer")` rows in
// `crates/perry-codegen/src/lower_call/native_table/http.rs` describe a
// valid dispatch. The runtime now routes through
// `js_ext_http_server_dispatch_method`, which mirrors the
// `("http", "HttpServer", ...)` native_table rows and returns the server
// handle from chainable methods.

import { createServer } from "node:http";

function makeServer(): any {
  return createServer((req: any, res: any) => { res.end(); });
}
const server: any = makeServer();

console.log("listen returns server:", server.listen(0) === server);

const onResult = server.on("listening", () => {});
console.log("on returns server:", onResult === server);

const addr = server.address();
console.log("address typeof:", typeof addr);
console.log("address.family:", addr && addr.family);
console.log("address.address:", addr && addr.address);
console.log("address.port typeof:", addr && typeof addr.port);

const closeResult = server.close();
console.log("close returns server:", closeResult === server);
