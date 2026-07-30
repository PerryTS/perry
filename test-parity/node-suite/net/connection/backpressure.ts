import * as net from "node:net";

const server = net.createServer((socket) => socket.resume());
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = new net.Socket({ writableHighWaterMark: 4 });
  client.connect((server.address() as any).port, "127.0.0.1");
  const accepted = client.write("abcdefgh");
  console.log(
    "queued:",
    accepted,
    client.writableLength >= 4,
    client.bufferSize >= 4,
  );
  client.destroy();
  await new Promise<void>((resolve) => client!.once("close", resolve));
} finally {
  client?.destroy();
  client?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
