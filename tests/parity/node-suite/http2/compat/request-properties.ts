import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const received = new Promise<void>((resolve) => {
    server.on("request", (request: any, response: any) => {
      console.log(
        request.method,
        request.url,
        request.httpVersion,
        request.headers[":path"],
      );
      response.statusCode = 204;
      response.end();
      resolve();
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const completed = new Promise<void>((resolve, reject) => {
    const request = client.request({ ":method": "PATCH", ":path": "/compat" });
    request.on("error", reject);
    request.on("end", resolve);
    request.resume();
    request.end();
  });
  await Promise.all([received, completed]);
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
