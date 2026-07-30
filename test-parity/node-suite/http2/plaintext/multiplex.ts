import * as http2 from "node:http2";

const server = http2.createServer();
let client: any;
try {
  const paths: string[] = [];
  server.on("stream", (stream: any, headers: any) => {
    paths.push(headers[":path"]);
    stream.respond({ ":status": 204 });
    stream.end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  await Promise.all(
    ["/one", "/two"].map((path) =>
      new Promise<void>((resolve, reject) => {
        const request = client.request({ ":path": path });
        request.on("error", reject);
        request.on("end", resolve);
        request.resume();
        request.end();
      })
    ),
  );
  console.log(paths.sort().join(","));
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
