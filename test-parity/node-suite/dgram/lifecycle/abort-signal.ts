import * as dgram from "node:dgram";

let invalidSignalSocket: dgram.Socket | undefined;
function invalidSignalResult(): string {
  try {
    invalidSignalSocket = dgram.createSocket({ type: "udp4", signal: {} as AbortSignal });
    return "accepted";
  } catch (error: unknown) {
    return (error as { code?: string }).code ?? "Error";
  }
}

console.log("invalid signal:", invalidSignalResult());
if (invalidSignalSocket) {
  await new Promise<void>((resolve) => invalidSignalSocket!.close(() => resolve()));
}

const controller = new AbortController();
const socket = dgram.createSocket({ type: "udp4", signal: controller.signal });
let closes = 0;
socket.on("close", () => closes++);
await new Promise<void>((resolve) => socket.close(() => resolve()));
controller.abort();
await new Promise<void>((resolve) => queueMicrotask(resolve));
console.log("abort after close:", closes);
