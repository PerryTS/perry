import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 204 });
    stream.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const controller = new AbortController();
  controller.abort();
  await new Promise<void>((resolve) => {
    const request = client.request({}, { signal: controller.signal });
    request.on("error", (error: any) => {
      console.log("error:", error.name, error.code);
      resolve();
    });
    request.on("end", () => {
      console.log("completed");
      resolve();
    });
    request.resume();
    request.end();
  });
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
