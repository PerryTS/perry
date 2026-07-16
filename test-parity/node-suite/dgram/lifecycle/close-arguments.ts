import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp4");
let closeEvents = 0;
socket.on("close", () => closeEvents++);
const closed = new Promise<void>((resolve) => {
  socket.once("close", () => queueMicrotask(resolve));
});

const result = (socket.close as (callback?: unknown) => dgram.Socket)("not a callback");
await closed;

console.log("close result self:", result === socket);
console.log("close events:", closeEvents);
