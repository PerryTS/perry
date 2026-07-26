import * as net from "node:net";

const sockets = new Set<net.Socket>();
const received: string[] = [];
const server = net.createServer({ pauseOnConnect: true }, (socket) => {
  sockets.add(socket);
  console.log("accepted paused:", socket.isPaused());
  socket.on("data", (data) => received.push(data.toString()));
  socket.on("close", () => sockets.delete(socket));
  socket.resume();
});

let client: net.Socket | undefined;
try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.on("error", () => {});
  client.end("hello");
  await new Promise<void>((resolve) => client!.once("close", resolve));
  console.log("received:", received.join(""));
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
