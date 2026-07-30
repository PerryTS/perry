import * as net from "node:net";

const sockets = new Set<net.Socket>();
const clients = new Set<net.Socket>();
const server = net.createServer((socket) => {
  sockets.add(socket);
  socket.on("close", () => sockets.delete(socket));
  socket.end();
});

async function connect(
  make: (port: number, callback: () => void) => net.Socket,
) {
  let client!: net.Socket;
  await new Promise<void>((resolve, reject) => {
    client = make((server.address() as any).port, function (this: any) {
      console.log("callback:", this === client, client.connecting);
      resolve();
    });
    clients.add(client);
    client.once("close", () => clients.delete(client));
    client.once("error", reject);
  });
  await new Promise<void>((resolve) => client.once("close", resolve));
}

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  await connect((port, callback) => net.connect(port, "127.0.0.1", callback));
  await connect((port, callback) =>
    net.connect({ port, host: "127.0.0.1" }, callback)
  );
  await connect((port, callback) =>
    new net.Socket().connect(port, "127.0.0.1", callback)
  );
} finally {
  for (const client of clients) {
    client.destroy();
    client.removeAllListeners();
  }
  for (const socket of sockets) {
    socket.destroy();
    socket.removeAllListeners();
  }
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
