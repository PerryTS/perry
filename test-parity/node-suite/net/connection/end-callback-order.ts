import * as net from "node:net";

const server = net.createServer((socket) => socket.resume());
const events: string[] = [];
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.on("connect", () => {
    events.push("connect");
    client!.end("x", () => events.push("end-callback"));
  });
  client.on("finish", () => events.push("finish"));
  client.on("close", () => events.push("close"));
  client.on("error", () => events.push("error"));
  await new Promise<void>((resolve) => client!.once("close", resolve));
  console.log(events.join(","));
} finally {
  client?.destroy();
  client?.removeAllListeners();
  if (server.listening) {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
  server.removeAllListeners();
}
