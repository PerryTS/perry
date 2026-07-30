import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const received = new Promise<void>((resolve) => {
    server.on("request", (request: any, response: any) => {
      let body = "";
      request.setEncoding("utf8");
      request.on("data", (chunk: string) => body += chunk);
      request.on("end", () => {
        console.log(body);
        response.statusCode = 204;
        response.end();
        resolve();
      });
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const request = client.request({ ":method": "POST" });
  request.end("hello h2");
  await received;
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
