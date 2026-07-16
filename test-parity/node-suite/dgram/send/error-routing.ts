import * as dgram from "node:dgram";

const withCallback = dgram.createSocket("udp4");
let callbackErrorEvents = 0;
withCallback.on("error", () => callbackErrorEvents++);
const callbackCode = await new Promise<string>((resolve) => {
  withCallback.send("x", 12345, "missing.invalid", (error) => resolve(error?.code ?? "none"));
});
await new Promise<void>((resolve) => queueMicrotask(resolve));
console.log("callback route:", callbackCode, callbackErrorEvents);
await new Promise<void>((resolve) => withCallback.close(() => resolve()));

const withEvent = dgram.createSocket("udp4");
const eventCode = new Promise<string>((resolve) => {
  withEvent.once("error", (error) => resolve(error.code));
});
withEvent.send("x", 12345, "missing.invalid");
console.log("event route:", await eventCode);
await new Promise<void>((resolve) => withEvent.close(() => resolve()));
