// Upstream: Node v26.5.0 lib/dgram.js Socket methods read receiver-owned state.
// Coverage added: invalid call/apply receivers on stateful methods.
import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    const value = error as { code?: string; name?: string };
    return value.code ?? value.name ?? "Error";
  }
}

const socket = dgram.createSocket("udp4");

try {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  const other = {} as dgram.Socket;
  console.log("address receiver:", codeOf(() => socket.address.call(other)));
  console.log("ref receiver:", codeOf(() => socket.ref.call(other)));
} finally {
  await new Promise<void>((resolve) => socket.close(resolve));
}
