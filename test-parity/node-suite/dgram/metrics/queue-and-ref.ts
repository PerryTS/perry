import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp4");
console.log("initial queue:", socket.getSendQueueSize(), socket.getSendQueueCount());
console.log("unbound identities:", socket.unref() === socket, socket.ref() === socket);

await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", () => resolve()));
console.log("bound identities:", socket.unref() === socket, socket.ref() === socket);
console.log("bound queue:", socket.getSendQueueSize(), socket.getSendQueueCount());

await new Promise<void>((resolve) => socket.close(() => resolve()));
console.log("closed identities:", socket.unref() === socket, socket.ref() === socket);
