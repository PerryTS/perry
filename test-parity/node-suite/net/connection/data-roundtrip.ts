import * as net from "node:net";

let accepted: net.Socket | undefined;
const server = net.createServer((socket) => {
  accepted = socket;
  socket.once("data", (data) => socket.end(data.toString().toUpperCase()));
});
let client: net.Socket | undefined;
let response = "";

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.setEncoding("utf8");
  client.on("data", (data) => response += data);
  client.on("error", () => {});
  client.end("perry");
  await new Promise<void>((resolve) => client!.once("close", resolve));
  console.log(response);
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
