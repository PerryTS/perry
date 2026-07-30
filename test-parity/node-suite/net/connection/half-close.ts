import * as net from "node:net";

let accepted: net.Socket | undefined;
const server = net.createServer(
  { allowHalfOpen: true },
  (socket) => accepted = socket,
);
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const connection = new Promise<void>((resolve) =>
    server.once("connection", () => resolve())
  );
  client = net.connect({
    port: (server.address() as any).port,
    host: "127.0.0.1",
    allowHalfOpen: true,
  });
  await new Promise<void>((resolve, reject) => {
    client!.once("connect", resolve);
    client!.once("error", reject);
  });
  await connection;
  console.log(
    "allow half open:",
    client.allowHalfOpen,
    accepted!.allowHalfOpen,
  );
  accepted!.end();
  console.log(
    "after end:",
    accepted!.writableEnded,
    accepted!.destroyed,
    client.destroyed,
  );
  client.destroy();
  await new Promise<void>((resolve) => client!.once("close", resolve));
} finally {
  client?.destroy();
  client?.removeAllListeners();
  accepted?.destroy();
  accepted?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
