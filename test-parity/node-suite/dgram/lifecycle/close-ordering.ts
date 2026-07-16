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

socket.close(() => order.push("callback"));
await closed;
await Promise.resolve();
console.log("close order:", order.join(","));
