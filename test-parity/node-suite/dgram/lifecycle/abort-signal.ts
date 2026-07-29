import * as dgram from "node:dgram";

let invalidSignalSocket: dgram.Socket | undefined;
function invalidSignalResult(): string {
  try {
    invalidSignalSocket = dgram.createSocket({
      type: "udp4",
      signal: {} as AbortSignal,
    });
    return "accepted";
  } catch (error: unknown) {
    return (error as { code?: string }).code ?? "Error";
  }
}

console.log("invalid signal:", invalidSignalResult());
if (invalidSignalSocket) {
  await new Promise<void>((resolve) =>
    invalidSignalSocket!.close(() => resolve())
  );
}

const preAbortedController = new AbortController();
preAbortedController.abort();
const preAbortedSocket = dgram.createSocket({
  type: "udp4",
  signal: preAbortedController.signal,
});
await new Promise<void>((resolve) => preAbortedSocket.once("close", resolve));
console.log("pre-aborted signal: closed");

const activeController = new AbortController();
const activeSocket = dgram.createSocket({
  type: "udp4",
  signal: activeController.signal,
});
const activeClosed = new Promise<void>((resolve) =>
  activeSocket.once("close", resolve)
);
activeController.abort();
await activeClosed;
console.log("active signal: closed");

const controller = new AbortController();
const socket = dgram.createSocket({ type: "udp4", signal: controller.signal });
let closes = 0;
socket.on("close", () => closes++);
await new Promise<void>((resolve) => socket.close(() => resolve()));
controller.abort();
await new Promise<void>((resolve) => queueMicrotask(resolve));
console.log("abort after close:", closes);
