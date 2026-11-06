// An http request body read through the web stream adapters, as fetch-style
// servers do: Readable.toWeb(req) read with a reader, the same converted back
// with Readable.fromWeb() and piped through a PassThrough. Every route must
// see the whole body.
import http from "node:http";
import { Readable, PassThrough } from "node:stream";
// A fetch-style server links the web streams the adapters convert to (Response, ReadableStream);
// the import is what links them here.
import "node:stream/web";

const BODY_BYTES = 300000;

const routes: Record<string, (req: http.IncomingMessage) => Promise<number>> = {
  async "/reader"(req) {
    const reader = Readable.toWeb(req).getReader();
    let n = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      n += (value as Uint8Array).length;
    }
    return n;
  },
  async "/roundtrip"(req) {
    const proxy = new PassThrough();
    Readable.fromWeb(Readable.toWeb(req) as any).pipe(proxy);
    let n = 0;
    for await (const chunk of proxy) n += chunk.length;
    return n;
  },
};

const server = http.createServer(async (req, res) => {
  try {
    const n = await routes[req.url as string](req);
    res.writeHead(200, { "Content-Type": "text/plain" });
    res.end(String(n));
  } catch (e) {
    res.writeHead(500, { "Content-Type": "text/plain" });
    res.end("error: " + (e as Error).message);
  }
});

const post = (port: number, path: string) =>
  new Promise<string>((resolve, reject) => {
    const req = http.request(
      { host: "127.0.0.1", port, path, method: "POST" },
      (res) => {
        let text = "";
        res.on("data", (c) => (text += c));
        res.on("end", () => resolve(text));
      },
    );
    req.on("error", reject);
    req.end(Buffer.alloc(BODY_BYTES, 97));
  });

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const path of Object.keys(routes)) {
    console.log(path, await post(port, path));
  }
  server.close();
  process.exit(0);
});
