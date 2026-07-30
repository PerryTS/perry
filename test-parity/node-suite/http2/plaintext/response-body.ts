import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  server.on("stream", (stream: any) => {
    stream.respond({ ":status": 200 });
    stream.end("hello h2");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  await new Promise<void>((resolve, reject) => {
    const request = client.request();
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk: string) => body += chunk);
    request.on("error", reject);
    request.on("end", () => {
      console.log(body);
      resolve();
    });
    request.end();
  });
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
