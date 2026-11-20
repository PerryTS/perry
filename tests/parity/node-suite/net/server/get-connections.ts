import * as net from "node:net";

const sockets = new Set<net.Socket>();
let acceptedClosed!: () => void;
const connectionClosed = new Promise<void>((resolve) =>
  acceptedClosed = resolve
);
const server = net.createServer((socket) => {
  sockets.add(socket);
  socket.on("close", () => {
    sockets.delete(socket);
    acceptedClosed();
  });
});

function count() {
  return new Promise<number>((resolve, reject) => {
    const result = server.getConnections((error, value) =>
      error ? reject(error) : resolve(value)
    );
    console.log("return:", result === server);
  });
}

let client: net.Socket | undefined;
try {
  console.log("before:", await count());
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const accepted = new Promise<void>((resolve) =>
    server.once("connection", () => resolve())
  );
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.on("error", () => {});
  await accepted;
  console.log("connected:", await count());
  client.destroy();
  await new Promise<void>((resolve) => client!.once("close", () => resolve()));
  await connectionClosed;
  console.log("closed:", await count());
} finally {
  client?.destroy();
  client?.removeAllListeners();
  for (const socket of sockets) {
    socket.destroy();
    socket.removeAllListeners();
  }
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
