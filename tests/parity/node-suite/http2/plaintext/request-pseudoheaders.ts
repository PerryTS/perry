import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const received = new Promise<void>((resolve) => {
    server.on("stream", (stream: any, headers: any) => {
      console.log(
        headers[":method"],
        headers[":path"],
        headers[":scheme"],
        typeof headers[":authority"],
      );
      stream.respond({ ":status": 204 });
      stream.end();
      resolve();
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const request = client.request({ ":method": "PUT", ":path": "/items?x=1" });
  request.resume();
  request.end();
  await received;
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
