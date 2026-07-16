import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");
const message = Buffer.from("--slice--");

const received = new Promise<string>((resolve) => {
  receiver.once("message", (value) => resolve(value.toString()));
});
const callback = new Promise<string>((resolve) => {
  sender.send(message, 2, 5, receiver.address().port, "127.0.0.1", (error, bytes) => {
    resolve(`${error === null}:${bytes}`);
  });
});

console.log("range message:", await received);
console.log("range callback:", await callback);
await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
