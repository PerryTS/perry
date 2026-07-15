import * as dgram from "node:dgram";

const receiver = dgram.createSocket("udp4");
await new Promise<void>((resolve) => receiver.bind(0, "127.0.0.1", () => resolve()));
const sender = dgram.createSocket("udp4");

let synchronous = true;
let callbackWasAsync = false;
let callbackBytes = -1;
let callbackError = "unset";

const received = new Promise<string>((resolve) => {
  receiver.once("message", (message) => resolve(message.toString()));
});
const sent = new Promise<void>((resolve) => {
  sender.send("callback", receiver.address().port, "127.0.0.1", (error, bytes) => {
    callbackWasAsync = !synchronous;
    callbackBytes = bytes;
    callbackError = error === null ? "null" : error.code;
    resolve();
  });
});
synchronous = false;

await Promise.all([received, sent]);
console.log("callback async:", callbackWasAsync);
console.log("callback result:", callbackError, callbackBytes);
console.log("message:", await received);

await Promise.all([
  new Promise<void>((resolve) => sender.close(() => resolve())),
  new Promise<void>((resolve) => receiver.close(() => resolve())),
]);
