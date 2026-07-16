import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string }).code ?? "Error";
  }
}

const socket = dgram.createSocket("udp4");
await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", () => resolve()));

console.log(
  "ttl invalid:",
  [0, 256, Infinity, "64"].map((value) => codeOf(() => socket.setTTL(value as never))).join(","),
);
console.log(
  "multicast ttl invalid:",
  [-1, 256, Infinity, "64"]
    .map((value) => codeOf(() => socket.setMulticastTTL(value as never)))
    .join(","),
);
console.log("ttl valid:", socket.setTTL(64), socket.setMulticastTTL(0));
await new Promise<void>((resolve) => socket.close(() => resolve()));
