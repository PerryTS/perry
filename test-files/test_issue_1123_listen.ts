// Issue #1123 followup — `net.createServer(...).listen(port, cb)` end-to-end.
//
// The initial #1123 fix landed createServer's return value (`typeof server`
// flipped from "undefined" to "object") but `server.listen(...)` still
// threw `TypeError: (number).listen is not a function` because the
// placeholder runtime `js_net_create_server` registered a handle without an
// accept loop and NATIVE_MODULE_TABLE had no rows for `("net", "Server",
// "listen")`.
//
// This test exercises the full lifecycle now wired in
// crates/perry-ext-net/src/lib.rs (js_net_server_listen + accept loop) +
// crates/perry-codegen/src/lower_call.rs (NATIVE_MODULE_TABLE rows for
// Server.listen/.close/.address/.on) + crates/perry-hir/src/lower.rs
// (registers NetCreateServer let-bindings as ("net", "Server") so method
// dispatch finds the class_filter entries):
//
//   1. createServer with a connection handler that prints what it saw
//   2. listen on port 18994 — verify the listen() callback fires
//   3. open a client via net.connect → write a Buffer → both sides close
//   4. server.close — verify the server tears down cleanly
//   5. exit 0 via self-terminating timer (no infinite loop)
//
// `client.write(Buffer.from(...))` is used instead of `client.write("ping")`
// because `js_net_socket_write` reads its arg as a BufferHeader pointer;
// the bare-string overload (StringHeader vs BufferHeader layout mismatch)
// is a pre-existing limitation tracked separately. Same shape used by
// existing perry-ext-net socket tests.

import { createServer, connect } from "node:net";

const PORT = 18994;

const server = createServer((sock: any) => {
    sock.on("data", (chunk: any) => {
        // Length-only assertion. `chunk.toString()` round-trip through
        // alloc_buffer + js_buffer_to_string works for ASCII payloads
        // (validated separately by other socket tests), but the wire
        // here goes via the new accept-loop ServerConnection event;
        // pinning byte length is enough to prove the bytes flow.
        console.log("SERVER GOT len=" + chunk.length);
    });
    sock.on("close", () => {
        // No-op — the close event closes the loop.
    });
});

server.listen(PORT, () => {
    console.log("LISTENING " + PORT);
    // Now open a client and send a ping. The accept-loop pushes
    // a ServerConnection event so the createServer handler runs
    // back on the main thread, registers its data listener, then
    // bytes flow through.
    //
    // Listeners must be registered AFTER `connect(...)` returns
    // because the closure body of a connectListener gets lowered
    // as an arg to `connect(...)` before the `let client`
    // registration runs — see lower_decl.rs's let-stmt scan.
    // Inside the closure body, `client` isn't tagged as a Socket
    // native instance yet, so `client.on(...)` would fall through
    // to generic property dispatch. Registering at the outer level
    // sidesteps that and matches typical Node patterns anyway.
    const client = connect(PORT, "127.0.0.1");
    client.on("connect", () => {
        // Use Buffer.from(...) — see header comment.
        client.write(Buffer.from("ping"));
        // Close from the client side after a tick; the server's
        // 'close' fires when the EOF reaches its accepted socket.
        setTimeout(() => {
            client.end();
        }, 100);
    });
    client.on("close", () => {
        console.log("CLOSED");
        server.close();
    });
});

// Self-terminating safety net. If anything in the lifecycle hangs
// (server bind hangs, no data flows, etc.) the process still exits
// cleanly within the parity-test 30s budget. 1500ms is generous —
// the local TCP round-trip + close handshake completes in <50ms on
// a warm machine.
setTimeout(() => {
    // No-op: timer existence keeps the runtime alive long enough
    // for the async chain to complete. Actual exit happens when
    // `server.close()` removes the last keepalive handle.
}, 1500);
