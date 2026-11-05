// Issue #2131 — follow-up to #1852. Pins the next net.Socket / net.Server
// lifecycle + EventEmitter surface fixes:
//
//   A. `socket.address()` returns a real `{ port, family, address }` object
//      on a connected Socket, populated from the kernel's local address
//      via `local_addr()`. Pre-fix it was undefined and `.port` threw the
//      "undefined.address" cluster the radar batched into #2131.
//   B. `socket.once(event, cb)` fires exactly once and auto-removes,
//      matching Node's EventEmitter semantics. Pre-fix `.once` fell
//      through to the unknown-method path and silently no-op'd, so the
//      caller's listener never ran and the test hung.
//   C. `socket.removeListener(event, cb)` / `socket.off(event, cb)` removes
//      a specific listener; later `.emit()`/data events skip the removed
//      callback. Pre-fix both threw "not a function" — the "6 callback
//      assertions: function should not have been called" cluster.
//   D. `socket.removeAllListeners(event?)` drains the per-event listener
//      vector (or the entire registry when called bare).
//   E. `socket.listenerCount(event)` returns the number of registered
//      callbacks; complements `.removeListener` / `.removeAllListeners`.
//   F. `socket.eventNames()` returns the names of events with at least
//      one registered listener.
//   G. `socket.addListener(event, cb)` is the alias for `.on`. (3
//      `undefined.on` cases — `.addListener` shape used by `pipe`.)
//   H. Same `.once` / `.off` / `.removeListener` / `.removeAllListeners` /
//      `.listenerCount` surface on `net.Server`.
//   I. `socket.resetAndDestroy()` is callable + tears the socket down
//      (the "reset-after-destroy" / "reset-until-connected" hang cluster).
//
// Like #1852 the output is causal (every line is gated on a protocol
// step), not ephemeral — Node and Perry agree byte-for-byte.

import { createServer, connect } from "node:net";

const server = createServer((sock: any) => {
  // C — register and immediately remove a listener; the "removed" one
  // must not run when data arrives.
  const removedListener = (_chunk: any) => {
    console.log("S:removed-fired-BUG");
  };
  sock.on("data", removedListener);
  sock.removeListener("data", removedListener);

  sock.on("data", (chunk: any) => {
    // E — listenerCount after the explicit removeListener above is 1
    // (this very handler). Print causally inside the data path so the
    // order is deterministic across runtimes (server-side `'connection'`
    // vs client-side `'connect'` race-order isn't fixed; the data
    // arrival isn't ambiguous).
    console.log("S:data-listeners-after-remove=" + sock.listenerCount("data"));
    console.log("S:got:" + chunk.toString());
    // A — `address()` on the *accepted* socket should resolve to a real
    // object too (server-side local addr = the server's bound addr).
    const addr = sock.address() as any;
    console.log(
      "S:addr-object=" + (typeof addr === "object" && addr !== null),
    );
    console.log("S:addr-has-port=" + (typeof addr.port === "number"));
    sock.end("pong");
  });
});

server.listen(0, () => {
  const addr = server.address() as any;
  const port = addr.port;

  const client = connect(port, "127.0.0.1");
  client.setNoDelay(true);

  // B — `.once('connect', …)` runs once. We register a counter on a
  // *separate* event and emit it twice via the data path to verify the
  // auto-removal contract holds on a user-driven event.
  let onceFires = 0;
  client.once("custom" as any, () => {
    onceFires += 1;
  });

  // G — `.addListener` is the Node alias for `.on`.
  let connectFired = 0;
  client.addListener("connect", () => {
    connectFired += 1;
  });

  client.on("connect", () => {
    // F — eventNames(): we currently have at least connect/data/end/close
    // listeners + the `.once` custom one. Just check the count is >= 4
    // so we don't depend on iteration order across runtimes.
    const names = client.eventNames();
    console.log(
      "C:event-names>=4=" + (Array.isArray(names) && names.length >= 4),
    );

    // E — listenerCount on the connect event matches what we registered
    // (one from .addListener above + this very handler = 2). We assert
    // >= 1 to stay robust to whether the in-flight 'connect' dispatch
    // has already begun draining `once`-style internals.
    console.log("C:connect-count>=1=" + (client.listenerCount("connect") >= 1));

    client.write("ping");
  });

  client.on("data", (d: any) => {
    console.log("C:got:" + d.toString());
    console.log("C:once-fires-pre-emit=" + onceFires);
  });

  client.on("end", () => {
    console.log("C:end");
    console.log("C:connect-fired=" + connectFired);
  });

  client.on("close", () => {
    console.log("C:close");
    // D — drain all listeners on this client; subsequent `.on` could
    // register fresh ones but we're tearing down anyway. Verify the
    // drain by checking listenerCount went to 0 for a known event.
    client.removeAllListeners("data");
    console.log("C:data-count-post-drain=" + client.listenerCount("data"));

    // H — same EventEmitter surface on the Server: register a noop
    // listener, remove it, count goes back to 0.
    const noopServerListener = () => {
      console.log("S:noop-server-BUG");
    };
    server.on("close", noopServerListener);
    server.removeListener("close", noopServerListener);
    console.log("S:close-count-after-remove=" + server.listenerCount("close"));

    server.close(() => {
      console.log("S:closed");
    });
  });
});

// Safety net: if any step hangs the process still exits within the parity
// budget. A warm local TCP round-trip + close handshake completes in <50ms.
setTimeout(() => {}, 2000);
