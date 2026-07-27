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
  for (const key of [":status", ":protocol", ":unknown"]) {
    try {
      const request = client.request({ [key]: "value" });
      console.log(key, "accepted");
      request.destroy();
    } catch (error: any) {
      console.log(key, error.name, error.code);
    }
  }
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
