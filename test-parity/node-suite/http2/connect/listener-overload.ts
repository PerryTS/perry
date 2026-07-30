import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  server.on("session", () => {});
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  await new Promise<void>((resolve, reject) => {
    client = http2.connect(
      `http://127.0.0.1:${(server.address() as any).port}`,
      {},
      () => {
        console.log("listener called:", client.connecting);
        resolve();
      },
    );
    client.on("error", reject);
  });
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
