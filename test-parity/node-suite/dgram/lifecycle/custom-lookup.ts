import * as dgram from "node:dgram";

const calls: string[] = [];
const socket = dgram.createSocket({
  type: "udp4",
  lookup(hostname, family, callback) {
    calls.push(`${hostname}:${family}`);
    callback(null, "0.0.0.0", 4);
  },
});

await new Promise<void>((resolve) => socket.bind(0, () => resolve()));
console.log("lookup calls:", calls.length, calls[0]?.startsWith("0.0.0.0:") ?? false);
await new Promise<void>((resolve) => socket.close(() => resolve()));
