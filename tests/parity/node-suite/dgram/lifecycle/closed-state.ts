// Upstream: Node v26.5.0 lib/dgram.js healthCheck() and Socket.prototype.close().
// Coverage added: repeated close and safe core operations after close.
import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string; name?: string }).code ??
      (error as { name?: string }).name ?? "Error";
  }
}

const socket = dgram.createSocket("udp4");
let isClosed = false;
socket.once("close", () => isClosed = true);
try {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve) => socket.close(resolve));
  console.log("repeat close:", codeOf(() => socket.close()));
  console.log("address closed:", codeOf(() => socket.address()));
  console.log("bind closed:", codeOf(() => socket.bind(-1, "127.0.0.1")));
  console.log("control closed:", codeOf(() => socket.setBroadcast(true)));
} finally {
  if (!isClosed) {
    await new Promise<void>((resolve) => socket.close(resolve));
  }
}
