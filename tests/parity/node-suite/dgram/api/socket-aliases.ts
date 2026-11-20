// Upstream: Node v26.5.0 lib/dgram.js Socket inheritance and public surface.
// Coverage added: EventEmitter aliases and the absent hasRef method.
import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp4");

try {
  console.log(
    "aliases:",
    socket.on === socket.addListener,
    socket.off === socket.removeListener,
  );
  console.log(
    "hasRef:",
    typeof (socket as dgram.Socket & { hasRef?: unknown }).hasRef,
  );
} finally {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve) => socket.close(resolve));
}
