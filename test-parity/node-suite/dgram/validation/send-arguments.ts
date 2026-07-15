import * as dgram from "node:dgram";

function codeOf(fn: () => unknown): string {
  try {
    fn();
    return "none";
  } catch (error: unknown) {
    return (error as { code?: string; name?: string }).code ??
      (error as { name?: string }).name ?? "Error";
  }
}

const socket = dgram.createSocket("udp4");
const buffer = Buffer.from("hello");

console.log("missing message:", codeOf(() => socket.send()));
console.log("number message:", codeOf(() => socket.send(23, 12345, "127.0.0.1")));
console.log(
  "bad list member:",
  codeOf(() => socket.send([buffer, 23] as never, 12345, "127.0.0.1")),
);
console.log("port zero:", codeOf(() => socket.send(buffer, 0, "127.0.0.1")));
console.log("port high:", codeOf(() => socket.send(buffer, 65536, "127.0.0.1")));

await new Promise<void>((resolve) => socket.close(() => resolve()));
