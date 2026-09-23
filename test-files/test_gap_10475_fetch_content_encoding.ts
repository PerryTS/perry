// Gap test: #10475 — global fetch decodes gzip, deflate (zlib and raw), and
// Brotli response bodies, including stacked codings, while keeping the
// Content-Encoding header observable; and it sends undici's default
// `Accept-Encoding` (`gzip, deflate` over http:) unless the caller set one.
//
// Self-contained: the server is in-process on an ephemeral port. (The PR's
// first version read the port from `process.env.PORT` and expected an external
// server, which is why it failed parity in CI.)
import http from "node:http";
import zlib from "node:zlib";

const PLAIN = '{"compressed":true}';
const body = Buffer.from(PLAIN);

const routes: Record<string, [string, () => Buffer]> = {
  "/gzip": ["gzip", () => zlib.gzipSync(body)],
  "/deflate": ["deflate", () => zlib.deflateSync(body)],
  "/deflate-raw": ["deflate", () => zlib.deflateRawSync(body)],
  "/br": ["br", () => zlib.brotliCompressSync(body)],
  "/stacked": ["deflate, gzip", () => zlib.gzipSync(zlib.deflateSync(body))],
  "/upper": ["GZIP", () => zlib.gzipSync(body)],
  "/unknown": ["snappy", () => Buffer.from("raw-bytes")],
};

const server = http.createServer((req, res) => {
  const url = req.url ?? "/";
  if (url === "/accept") {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end(String(req.headers["accept-encoding"]));
    return;
  }
  const route = routes[url];
  if (!route) {
    res.writeHead(404);
    res.end();
    return;
  }
  const encoded = route[1]();
  res.writeHead(200, {
    "content-type": "application/json",
    "content-encoding": route[0],
    "content-length": String(encoded.length),
  });
  res.end(encoded);
});

server.listen(0, "127.0.0.1", async () => {
  const address = server.address() as { port: number };
  const base = `http://127.0.0.1:${address.port}`;
  try {
    for (const path of ["/gzip", "/deflate", "/deflate-raw", "/br", "/stacked", "/upper"]) {
      const response = await fetch(base + path);
      const text = await response.text();
      console.log(path, response.headers.get("content-encoding"), text === PLAIN, text.length);
    }
    const unknown = await fetch(base + "/unknown");
    console.log("/unknown", unknown.headers.get("content-encoding"), await unknown.text());

    console.log("json", JSON.stringify(await (await fetch(base + "/gzip")).json()));
    const buffer = await (await fetch(base + "/br")).arrayBuffer();
    console.log("arrayBuffer", buffer.byteLength);

    console.log("accept", await (await fetch(base + "/accept")).text());
    const custom = await fetch(base + "/accept", {
      headers: { "Accept-Encoding": "identity" },
    });
    console.log("custom", await custom.text());
  } catch (error: any) {
    console.log("error", error?.name, error?.message, error?.cause?.code);
  } finally {
    server.close();
  }
});
