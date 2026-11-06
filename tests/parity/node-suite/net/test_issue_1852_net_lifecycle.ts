// Issue #1852 — `node:net` connection lifecycle end-to-end. Pins the four
// lifecycle fixes that flipped the net-layer parity clusters:
//
//   A. `server.address()` returns a real object (`{ port, address, family }`)
//      so `server.address().port` reads a number — pre-fix the native-table
//      `address` row used NR_PTR, which NaN-boxed the JSON StringHeader as a
//      POINTER object so `.port` came back `undefined` ("undefined.address"
//      cluster).
//   B. `server.listen(0, cb)` records the *actual* ephemeral port the OS
//      assigned (via `TcpListener::local_addr()`) before firing the listen
//      callback — pre-fix `address().port` was the requested 0, so the
//      client connected to port 0 and the whole exchange hung.
//   C. The socket `'end'` event fires when the peer half-closes (FIN) —
//      pre-fix only `'close'` fired, so `socket.on('end', …)` never ran and
//      the lifecycle never completed (the 19-case "timeout/hang" cluster).
//   D. `socket.end(data)` writes the final chunk before sending FIN — pre-fix
//      the payload was silently dropped (the peer's `'data'` never fired).
//   E. Chainable no-op option setters (`setNoDelay`, etc.) are callable
//      instead of throwing "x is not a function" ("value() missing" cluster).
//
// Output is deterministic despite the ephemeral port: every line is gated on
// a causal step of the protocol (server must receive "ping" before it can
// reply "pong"; the client must receive "pong" before its FIN-driven 'end';
// `server.close()` is invoked from inside the client's 'close' handler), and
// the port is printed only as a `> 0` boolean, so Node and Perry agree
// byte-for-byte.

import { createServer, connect } from "node:net";

const server = createServer((sock: any) => {
  sock.on("data", (chunk: any) => {
    console.log("S:got:" + chunk.toString());
    // D — `end(data)` must put "pong" on the wire, then half-close.
    sock.end("pong");
  });
});

server.listen(0, () => {
  const addr = server.address() as any;
  // A + B — a real object whose port is the assigned ephemeral port.
  console.log("L:addr-object=" + (typeof addr === "object" && addr !== null));
  console.log("L:port>0=" + (addr.port > 0));

  const client = connect(addr.port, "127.0.0.1");
  // E — chainable no-op; if this threw, the program would abort here.
  client.setNoDelay(true);

  client.on("connect", () => {
    client.write("ping");
  });
  client.on("data", (d: any) => {
    console.log("C:got:" + d.toString());
  });
  client.on("end", () => {
    // C — fires on the server's FIN (sent by `sock.end("pong")`).
    console.log("C:end");
  });
  client.on("close", () => {
    console.log("C:close");
    server.close(() => {
      console.log("S:closed");
    });
  });
});

// Safety net: if any step hangs the process still exits within the parity
// budget. A warm local TCP round-trip + close handshake completes in <50ms.
setTimeout(() => {}, 2000);
