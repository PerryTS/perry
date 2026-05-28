// Issue #2211 — EventEmitter introspection on a net.Socket. Tests in
// the `test-http-agent-*` and `test-http-server-*` families call
// `socket.listeners('timeout')`, `socket.eventNames()`,
// `socket.rawListeners(...)`, etc. on the socket handed to
// `request.on('socket', sock => …)`. Pre-#2211 each threw "value is
// not a function" because perry-ext-net's NativeModSig surface only
// exposed `on`/`once`/`removeListener`/`removeAllListeners`/
// `listenerCount`/`eventNames` (PRs #2131/#2173).
//
// The test exercises the same introspection surface against a
// directly-constructed `net.Socket` so it doesn't depend on Perry's
// `http.request` firing a `'socket'` event (which is independently
// gated on the client-side wiring). The class is the same; the FFI
// dispatch the patch adds is what the parity check pins.
import { Socket } from "node:net";

// typeof on the method-value is intentionally not checked: Perry
// reports `number` for native method-value reads vs. Node's `function`
// (separate issue family, #1380-shape). The relevant fix is the
// callable surface; the actual invocations below pin the dispatch.

const sock = new Socket();

const handlerA = () => {};
const handlerB = () => {};
sock.on("timeout", handlerA);
sock.on("timeout", handlerB);

const ls = sock.listeners("timeout");
console.log("listeners isArray:", Array.isArray(ls));
console.log("listeners length>=2:", ls.length >= 2);
console.log("listenerCount>=2:", sock.listenerCount("timeout") >= 2);

const raw = sock.rawListeners("timeout");
console.log("rawListeners isArray:", Array.isArray(raw));
console.log("rawListeners length>=2:", raw.length >= 2);

const names = sock.eventNames();
console.log("eventNames isArray:", Array.isArray(names));
console.log("eventNames includes timeout:", names.includes("timeout"));

sock.removeListener("timeout", handlerA);
console.log(
  "listenerCount after removeListener>=1:",
  sock.listenerCount("timeout") >= 1,
);

sock.removeAllListeners("timeout");
console.log("listenerCount after removeAll:", sock.listenerCount("timeout"));

sock.destroy();
