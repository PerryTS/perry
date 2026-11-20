import * as net from "node:net";

let output = "";
const server = net.createServer((socket) => {
  socket.setEncoding("utf8");
  socket.on("data", (data) => output += data);
  socket.on("end", () => socket.end());
});
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  await new Promise<void>((resolve, reject) => {
    client!.once("connect", resolve);
    client!.once("error", reject);
  });
  client.cork();
  client.write("a");
  client.write("b");
  console.log("corked:", client.writableCorked, client.writableLength);
  client.uncork();
  console.log("uncorked:", client.writableCorked);
  client.end("c");
  await new Promise<void>((resolve) => client!.once("close", resolve));
  console.log("received:", output);
} finally {
  client?.destroy();
  client?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
