import * as http2 from "node:http2";

// `session.request()` ends the stream itself for a method with no payload
// (Node's default `endStream`), so the response arrives with no `end()` call;
// `endStream: true` does the same for any method, and an `end()` after the
// stream already ended is harmless (PerryTS/perry#14681).
const server = http2.createServer();
let client: any;
try {
  server.on("stream", (stream: any, headers: any) => {
    stream.respond({ ":status": 200 });
    if (headers[":method"] === "HEAD") stream.end();
    else stream.end(`${headers[":method"]} ok`);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
  await new Promise<void>((resolve, reject) => {
    client.on("error", reject);
    client.on("connect", resolve);
  });
  const fetch = (headers: any, options?: any, end?: boolean) =>
    new Promise<string>((resolve, reject) => {
      const request = client.request(headers, options);
      let text = "";
      request.setEncoding("utf8");
      request.on("data", (chunk: string) => text += chunk);
      request.on("error", reject);
      request.on("end", () => resolve(JSON.stringify(text)));
      if (end) request.end();
    });
  console.log(await fetch({ ":path": "/" }));
  console.log(await fetch({ ":method": "HEAD", ":path": "/" }));
  console.log(await fetch({ ":method": "DELETE", ":path": "/" }));
  console.log(await fetch({ ":method": "POST", ":path": "/" }, { endStream: true }));
  console.log(await fetch({ ":path": "/" }, undefined, true));
} finally {
  client?.destroy();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
