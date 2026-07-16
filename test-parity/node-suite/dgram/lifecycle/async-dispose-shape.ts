import * as dgram from "node:dgram";

const socket = dgram.createSocket("udp4");
console.log(
  "async dispose:",
  typeof (socket as unknown as Record<symbol, unknown>)[Symbol.asyncDispose],
);
await new Promise<void>((resolve) => socket.close(() => resolve()));
