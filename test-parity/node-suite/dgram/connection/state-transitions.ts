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
const firstPeer = dgram.createSocket("udp4");
const secondPeer = dgram.createSocket("udp4");
await Promise.all([
  new Promise<void>((resolve) => firstPeer.bind(0, "127.0.0.1", () => resolve())),
  new Promise<void>((resolve) => secondPeer.bind(0, "127.0.0.1", () => resolve())),
]);
const firstPort = firstPeer.address().port;
const secondPort = secondPeer.address().port;

console.log("disconnect before connect:", codeOf(() => socket.disconnect()));
console.log("bad ports:", [0, -1, 65536].map((port) => codeOf(() => socket.connect(port))).join(","));

const firstConnect = new Promise<void>((resolve) => {
  socket.connect(firstPort, "127.0.0.1", () => resolve());
});
console.log("connect while pending:", codeOf(() => socket.connect(firstPort)));
await firstConnect;

let remote = socket.remoteAddress();
console.log("first remote:", remote.address, remote.family, remote.port === firstPort);
console.log("connect while connected:", codeOf(() => socket.connect(secondPort)));
console.log("disconnect result:", socket.disconnect());
console.log("remote after disconnect:", codeOf(() => socket.remoteAddress()));
console.log("repeat disconnect:", codeOf(() => socket.disconnect()));

await new Promise<void>((resolve) => {
  socket.connect(secondPort, "127.0.0.1", () => resolve());
});
remote = socket.remoteAddress();
console.log("reconnected remote:", remote.address, remote.family, remote.port === secondPort);
await Promise.all([
  new Promise<void>((resolve) => socket.close(() => resolve())),
  new Promise<void>((resolve) => firstPeer.close(() => resolve())),
  new Promise<void>((resolve) => secondPeer.close(() => resolve())),
]);
