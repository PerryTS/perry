import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp4");
await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));

const order: string[] = [];
const closed = new Promise<void>((resolve) => {
  socket.once("close", () => {
    order.push("event");
    resolve();
  });
});

try {
  socket.close(function () {
    order.push(`callback:${this === socket}:${arguments.length}`);
  });
  await closed;
  await Promise.resolve();
  console.log("close order:", order.join(","));
} finally {
  if (!order.includes("event")) {
    await new Promise<void>((resolve) => socket.close(resolve));
  }
}
