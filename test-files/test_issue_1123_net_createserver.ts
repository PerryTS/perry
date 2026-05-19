// Issue #1123 — `net.createServer(...)` had two distinct defects on 0.5.1010:
//
// A. Dotted form (`import * as net from "node:net"; net.createServer(...)`)
//    bailed at codegen Phase 2 with
//    `expression NetCreateServer not yet supported`. The HIR lowering at
//    `crates/perry-hir/src/lower/expr_call.rs:1899` synthesized
//    `Expr::NetCreateServer { options, connection_listener }` but the LLVM
//    backend had no match arm for it (only perry-codegen-js and
//    perry-codegen-wasm did).
//
// B. Named-import form (`import { createServer } from "node:net";
//    createServer(...)`) compiled+linked but `server === undefined` at
//    runtime — the bare-call routing in expr_call.rs sent it through the
//    generic `Expr::NativeMethodCall` arm whose `NATIVE_MODULE_TABLE`
//    has no `("net", "createServer")` row, so the call fell through
//    every match and returned `TAG_UNDEFINED`.
//
// Fix:
// 1. `crates/perry-codegen/src/expr.rs` — added an `Expr::NetCreateServer`
//    match arm that calls the runtime symbol
//    `js_net_create_server(options_i64, listener_i64) -> DOUBLE`. Both
//    args are NaN-unboxed to raw `i64` pointers via `unbox_to_i64`; missing
//    options/listener slots pass `0` (the runtime tolerates a null pointer).
// 2. `crates/perry-hir/src/lower/expr_call.rs` — intercepted the named-import
//    bare-call resolution path so `("net", "createServer")` synthesizes
//    `Expr::NetCreateServer` instead of `Expr::NativeMethodCall`, making
//    both forms converge on the new codegen arm.
// 3. `crates/perry-ext-net/src/lib.rs` — added the runtime symbol
//    `js_net_create_server` (previously declared in `runtime_decls.rs:2690`
//    but never implemented anywhere in the linked libraries; the old
//    `perry-runtime/src/net.rs` referenced in the issue is gated off at
//    `lib.rs:79`). The implementation registers a placeholder handle and
//    stashes the connection listener under `'connection'`; the full
//    event-driven `server.listen` accept loop is a separate followup.
//
// What this test asserts: both call shapes return a non-undefined value
// of type `number` (the server handle). Full `.listen()` accept-loop
// behaviour is out of scope for this issue — `crates/perry-ext-net` has
// no `js_net_server_listen` yet — so we only pin the bare creation
// surface here. A future followup will add the accept loop + parity
// test against `node --experimental-strip-types`.

import * as net from "node:net";
import { createServer } from "node:net";

// A — dotted form. Pre-fix: codegen bail. Post-fix: Server handle.
const serverA = net.createServer((sock: any) => { sock.end("ok\n"); });
console.log("A typeof:", typeof serverA);
console.log("A truthy:", !!serverA);

// B — named-import form. Pre-fix: undefined. Post-fix: Server handle.
const serverB = createServer((sock: any) => { sock.end("ok\n"); });
console.log("B typeof:", typeof serverB);
console.log("B truthy:", !!serverB);

// The #1123 followup (v0.5.1012) changed `js_net_create_server` to
// return a NaN-boxed POINTER handle (matching `js_node_http_create_server`)
// so receiver-unboxing in `server.listen(...)` recovers the raw handle
// instead of masking it to 0. Side effect: `typeof === "object"` per
// Node's `net.Server extends EventEmitter`, and `===` / `>` comparisons
// on the raw value don't make sense anymore — drop those assertions.
// The actual listen + accept-loop behaviour is pinned in
// `test_issue_1123_listen.ts`.
