import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  await new Promise<void>((resolve, reject) => {
    client.settings(
      { initialWindowSize: 32768 },
      (error: any, settings: any) => {
        if (error) return reject(error);
        console.log(settings.initialWindowSize, client.pendingSettingsAck);
        resolve();
      },
    );
  });
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
