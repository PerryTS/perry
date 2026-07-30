import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 201, "x-test": "yes" });
    stream.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  await new Promise<void>((resolve, reject) => {
    const request = client.request();
    request.on("error", reject);
    request.on(
      "response",
      (headers: any) => console.log(headers[":status"], headers["x-test"]),
    );
    request.on("end", resolve);
    request.resume();
    request.end();
  });
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
