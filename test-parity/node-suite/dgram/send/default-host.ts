import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");

const first = new Promise<string>((resolve) => {
  receiver.once("message", (message) => resolve(message.toString()));
});
await new Promise<void>((resolve) => sender.send("implicit host", receiver.address().port, () => resolve()));
console.log("unconnected default host:", await first);

await new Promise<void>((resolve) => sender.connect(receiver.address().port, () => resolve()));
const second = new Promise<string>((resolve) => {
  receiver.once("message", (message) => resolve(message.toString()));
});
await new Promise<void>((resolve) => sender.send("connected host", () => resolve()));
console.log("connected default host:", await second);

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
