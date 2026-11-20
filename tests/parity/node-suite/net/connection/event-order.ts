import * as net from "node:net";

const server = net.createServer((socket) => socket.end("x"));
const events: string[] = [];
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect((server.address() as any).port, "127.0.0.1");
  client.on("connect", () => events.push("connect"));
  client.on("data", () => events.push("data"));
  client.on("end", () => events.push("end"));
  client.on("error", () => events.push("error"));
  client.on("close", (hadError) => events.push(`close:${hadError}`));
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
