// The turnloop HTTP/2 transport, asserted through the surface that changed.
//
// Two things here are the point rather than incidental coverage:
//
//   * `stream.id` is the RFC 9113 stream identifier of ITS OWN connection.
//     Perry used to hand out a process-global odd counter (`NEXT_H2_STREAM_ID`)
//     that corresponded to nothing on the wire, so a second session continued
//     5, 7, ... where Node restarts at 1, 3. Two sessions is the only shape
//     that tells the two apart.
//   * concurrent `session.request()` calls are multiplexed streams on one
//     connection. The `h2` client raced for a single `SendRequest` and the
//     loser got "HTTP/2 session is not connected".
//
// Everything else is the ordinary request/response surface over the new
// transport: a request body read to completion, a bodyless 204, and HEAD.
import * as http2 from "node:http2";

function get(client: any, headers: Record<string, string>, body?: string) {
  return new Promise<{ id: number; status: number; body: string }>((resolve, reject) => {
    const request = client.request(headers);
    let text = "";
    request.setEncoding("utf8");
    request.on("response", (h: any) => {
      request.on("data", (c: string) => (text += c));
      request.on("end", () => resolve({ id: request.id, status: h[":status"], body: text }));
    });
    request.on("error", reject);
    request.end(body);
  });
}

function connect(port: number): Promise<any> {
  return new Promise((resolve, reject) => {
    const client = http2.connect(`http://127.0.0.1:${port}`);
    client.on("error", reject);
    client.on("connect", () => resolve(client));
  });
}

const seen: string[] = [];
const server = http2.createServer((req: any, res: any) => {
  seen.push(req.method + " " + req.url);
  if (req.url === "/empty") {
    res.writeHead(204);
    res.end();
    return;
  }
  let body = "";
  req.setEncoding("utf8");
  req.on("data", (c: string) => (body += c));
  req.on("end", () => {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("echo:" + req.url + ":" + body);
  });
});

let a: any;
let b: any;
try {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as any).port;

  a = await connect(port);
  const first = await Promise.all([
    get(a, { ":path": "/one" }),
    get(a, { ":path": "/two" }),
  ]);
  console.log("session a ids " + first.map((r) => r.id).sort((x, y) => x - y).join(","));
  console.log("session a bodies " + first.map((r) => r.body).sort().join("|"));

  b = await connect(port);
  // `:method` matters: Node's `session.request()` infers `endStream` from it,
  // and a GET is end-of-stream the moment the HEADERS go out.
  const second = await get(b, { ":method": "POST", ":path": "/three" }, "payload");
  console.log("session b id " + second.id);
  console.log("session b body " + second.body);
  console.log("session b status " + second.status);

  const empty = await get(b, { ":path": "/empty" });
  console.log("204 status " + empty.status + " body " + JSON.stringify(empty.body));

  const head = await get(b, { ":method": "HEAD", ":path": "/head" });
  console.log("head status " + head.status + " body " + JSON.stringify(head.body));

  console.log("alpn " + a.alpnProtocol + " type " + a.type + " encrypted " + a.encrypted);
  console.log("seen " + seen.sort().join(","));
} finally {
  a?.close();
  b?.close();
  await new Promise<void>((resolve) => server.close(() => resolve()));
}
console.log("done");
