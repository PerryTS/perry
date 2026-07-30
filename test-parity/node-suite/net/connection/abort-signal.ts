import * as net from "node:net";

const server = net.createServer((socket) => socket.destroy());
const controller = new AbortController();
controller.abort();
const events: string[] = [];
let client: net.Socket | undefined;

try {
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  client = net.connect({
    port: (server.address() as any).port,
    host: "127.0.0.1",
    signal: controller.signal,
  });
  client.on("error", (error: any) => events.push(`error:${error.name}`));
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
