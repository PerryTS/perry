// stream/promises pipeline(source, res, { signal, end }) with a trailing options
// object (effect platform-node raw bodies): the object is options, not a stage.
import * as http from "node:http";
import * as NFS from "node:fs";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
NFS.writeFileSync("img.bin", Buffer.alloc(184, 3));
const lazyReadable = (evaluate: () => any, onDestroy?: () => void) => {
  let source: any;
  return new Readable({
    read() {
      if (source === void 0) {
        source = evaluate();
        source.on("data", (chunk: any) => { if (!this.push(chunk)) source.pause(); });
        source.once("end", () => this.push(null));
        source.once("error", (cause: any) => this.destroy(cause));
        source.once("close", () => { if (!source.readableEnded) this.destroy(new Error("Response stream closed prematurely")); });
      }
      source.resume();
    },
    destroy(error: any, callback: any) { source?.destroy(error ?? void 0); onDestroy?.(); callback(error); },
  });
};
const server = http.createServer((req, res) => {
  const mode = req.url.slice(1);
  const start = mode === "undef" ? undefined : 0;
  const body = lazyReadable(() => NFS.createReadStream("img.bin", { start, end: undefined }));
  res.writeHead(200, { "content-type": "image/png", "content-length": "184" });
  const ac = new AbortController();
  pipeline(body, res, { signal: ac.signal, end: true }).catch((err: any) => console.log("pipeline err", err.message));
});
server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const m of ["num", "undef"]) {
    const ac = new AbortController(); const t = setTimeout(() => ac.abort(), 3000);
    try { const r = await fetch(`http://127.0.0.1:${port}/${m}`, { signal: ac.signal }); const b = await r.arrayBuffer(); console.log(m, r.status, b.byteLength); }
    catch (e: any) { console.log(m, "failed", e.name); }
    clearTimeout(t);
  }
  server.close(); process.exit(0);
});
