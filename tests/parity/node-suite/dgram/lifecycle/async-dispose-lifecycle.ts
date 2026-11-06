// Upstream: Node v26.5.0 test/parallel/test-dgram-async-dispose.mjs.
// Coverage added: async disposal closure and promise settlement.
import * as dgram from "node:dgram";

type AsyncDisposableSocket = dgram.Socket & {
  [Symbol.asyncDispose]?: () => Promise<void>;
};

const socket = dgram.createSocket("udp4") as AsyncDisposableSocket;
await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
const dispose = socket[Symbol.asyncDispose];
if (typeof dispose !== "function") {
  console.log("async dispose supported:", false);
  await new Promise<void>((resolve) => socket.close(resolve));
} else {
  const order: string[] = [];
  socket.once("close", () => order.push("close"));
  try {
    await dispose.call(socket);
    order.push("resolved");
    console.log("async dispose supported:", true);
    console.log("async dispose order:", order.join(","));
  } finally {
    if (order.length === 0) socket.close();
  }
}
