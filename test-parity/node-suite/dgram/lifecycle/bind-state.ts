// Upstream: Node v26.5.0 test/parallel/test-dgram-bind.js.
// Coverage added: bind() return identity and already-bound validation order.
import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string; name?: string }).code ?? "Error";
  }
}

const socket = dgram.createSocket("udp4");
try {
  const listening = new Promise<void>((resolve) =>
    socket.once("listening", resolve)
  );
  console.log("bind result self:", socket.bind(0, "127.0.0.1") === socket);
  await listening;
  console.log("second bind:", codeOf(() => socket.bind(-1, "127.0.0.1")));
} finally {
  await new Promise<void>((resolve) => socket.close(resolve));
}
