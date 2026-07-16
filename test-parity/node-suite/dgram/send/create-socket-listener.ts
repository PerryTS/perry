import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4", (message, rinfo) => {
  console.log("constructor listener:", message.toString(), rinfo.family, rinfo.size);
});
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");

const received = new Promise<void>((resolve) => receiver.once("message", () => resolve()));
await new Promise<void>((resolve) => {
  sender.send("listener", receiver.address().port, "127.0.0.1", () => resolve());
});
await received;

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
