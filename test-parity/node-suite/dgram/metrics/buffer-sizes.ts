import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string }).code ?? "Error";
  }
}

const unbound = dgram.createSocket("udp4");
console.log("unbound send get:", codeOf(() => unbound.getSendBufferSize()));
console.log("unbound recv set:", codeOf(() => unbound.setRecvBufferSize(8192)));
await new Promise<void>((resolve) => unbound.close(() => resolve()));

const socket = dgram.createSocket("udp4");
await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", () => resolve()));
console.log(
  "invalid recv:",
  [-1, Infinity, "bad"].map((value) => codeOf(() => socket.setRecvBufferSize(value as never))).join(","),
);
console.log(
  "invalid send:",
  [-1, Infinity, "bad"].map((value) => codeOf(() => socket.setSendBufferSize(value as never))).join(","),
);
socket.setRecvBufferSize(10000);
socket.setSendBufferSize(10000);
console.log("positive sizes:", socket.getRecvBufferSize() > 0, socket.getSendBufferSize() > 0);
await new Promise<void>((resolve) => socket.close(() => resolve()));
