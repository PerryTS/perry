import * as net from "node:net";

let accepted: net.Socket | undefined;
const server = net.createServer((socket) => {
  accepted = socket;
  socket.on("data", () => {});
  socket.on("end", () => socket.end("reply"));
});
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.resume();
  client.on("error", () => {});
  client.end("request");
  await new Promise<void>((resolve) => client!.once("close", resolve));
  console.log(
    "client:",
    client.bytesRead,
    client.bytesWritten,
    client.bufferSize,
  );
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
