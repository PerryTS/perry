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

const peer = dgram.createSocket("udp4");
await new Promise<void>((resolve) => peer.bind(0, "127.0.0.1", () => resolve()));
const socket = dgram.createSocket("udp4");
await new Promise<void>((resolve) => {
  socket.connect(peer.address().port, "127.0.0.1", () => resolve());
});

console.log(
  "destination while connected:",
  codeOf(() => socket.send("x", peer.address().port, "127.0.0.1")),
);
console.log(
  "range destination while connected:",
  codeOf(() => socket.send(Buffer.from("x"), 0, 1, peer.address().port, "127.0.0.1")),
);

await Promise.all([
  new Promise<void>((resolve) => socket.close(() => resolve())),
  new Promise<void>((resolve) => peer.close(() => resolve())),
]);
