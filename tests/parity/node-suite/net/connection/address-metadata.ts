import * as net from "node:net";

let accepted: net.Socket | undefined;
const server = net.createServer((socket) => {
  accepted = socket;
  socket.resume();
});
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const connection = new Promise<void>((resolve) =>
    server.once("connection", () => resolve())
  );
  client = net.connect((server.address() as any).port, "127.0.0.1");
  await new Promise<void>((resolve, reject) => {
    client!.once("connect", resolve);
    client!.once("error", reject);
  });
  await connection;
  const clientAddress = client.address() as any;
  console.log(
    "client local:",
    clientAddress.address,
    clientAddress.family,
    typeof clientAddress.port,
  );
  console.log(
    "client remote:",
    client.remoteAddress,
    client.remoteFamily,
    client.remotePort === (server.address() as any).port,
  );
  console.log(
    "server local:",
    accepted!.localAddress,
    accepted!.localFamily,
    accepted!.localPort === (server.address() as any).port,
  );
  console.log(
    "server remote:",
    accepted!.remoteAddress,
    accepted!.remoteFamily,
    typeof accepted!.remotePort,
  );
  client.end();
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
