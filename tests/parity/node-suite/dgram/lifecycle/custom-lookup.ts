import * as dgram from "node:dgram";

const calls: string[] = [];
const socket = dgram.createSocket({
  type: "udp4",
  lookup(hostname, family, callback) {
    calls.push(`${hostname}:${family}`);
    callback(null, "127.0.0.1", 4);
  },
});

try {
  await new Promise<void>((resolve) => socket.bind(0, "127.0.0.1", resolve));
  console.log(
    "lookup calls:",
    calls.length,
    calls[0]?.startsWith("127.0.0.1:") ?? false,
  );
} finally {
  await new Promise<void>((resolve) => socket.close(resolve));
}
