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
//   3. open a client via net.connect → write a string → both sides close
//   4. server.close — verify the server tears down cleanly
//   5. exit 0 via self-terminating timer (no infinite loop)
//
// Issue #1131 regression: `client.write("ping")` (bare JS string, NOT
// a Buffer) must put the string's UTF-8 bytes on the wire. Pre-#1131
// `js_net_socket_write` read every chunk through the `BufferHeader`
// layout, so a `StringHeader`-shaped string surfaced its `utf16_len`
// as the length and pulled "data" from the middle of the header — the
// server saw garbage bytes (pre-fix the wire carried a 326-byte
// blob read out of the StringHeader's tail at the wrong offset)
// instead of the 4 UTF-8 bytes of "ping". This test now pins BOTH
// `len=4` and the decoded content `data=ping`, so it's the
// regression guard for #1131 as well as the #1123
// listen/accept-loop lifecycle.
//
// NOTE: the content assertion uses `chunk.toString()`, not
// `chunk[0]`. There is a *separate* pre-existing bug where integer
// indexing (`chunk[N]`) on a Buffer delivered through an
// `any`-typed `.on('data', cb)` callback parameter returns 0
// (`chunk.length` and `chunk.toString()` are correct; only
// `chunk[N]` is wrong). It reproduces with zero net code — a plain
// `function f(b: any) { return b[0] }` called with
// `Buffer.from("ping")` — so it is NOT part of #1131 (the outbound
// write path, which this asserts is byte-correct). Tracked
// separately. Using `toString()` keeps this test a true #1131
// regression guard: pre-#1131 it printed a long garbage string,
// post-#1131 it prints exactly `ping`.

import { createServer, connect } from "node:net";

const PORT = 18994;

const server = createServer((sock: any) => {
    sock.on("data", (chunk: any) => {
        // #1131 — assert the length AND the decoded UTF-8 content.
        // `data=ping` proves the string's actual bytes reached the
        // server. Pre-fix this printed a 326-char garbage blob
        // (StringHeader read through the BufferHeader layout).
        console.log(
            "SERVER GOT len=" + chunk.length + " data=" + chunk.toString(),
        );
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
        // #1131 — bare JS string, must arrive as UTF-8 bytes.
        client.write("ping");
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
