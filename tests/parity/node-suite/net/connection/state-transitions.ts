import * as net from "node:net";

const server = net.createServer((socket) => socket.end());
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.on("error", () => {});
  console.log(
    "opening:",
    client.connecting,
    client.pending,
    client.destroyed,
    client.readyState,
  );
  await new Promise<void>((resolve) => client!.once("connect", resolve));
  console.log(
    "open:",
    client.connecting,
    client.pending,
    client.destroyed,
    client.readyState,
  );
  await new Promise<void>((resolve) => client!.once("close", resolve));
  console.log(
    "closed:",
    client.connecting,
    client.pending,
    client.destroyed,
    client.readyState,
  );
} finally {
  client?.destroy();
  client?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
