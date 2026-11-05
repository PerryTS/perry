// An IncomingMessage body read four ways: flowing "data", paused-mode
// "readable" + read() (effect platform-node), async iteration, and the web
// stream adapter. Each reader gets the whole 184-byte body.
import * as http from "node:http";
import { Readable } from "node:stream";
const readVia: Record<string, (req: any) => Promise<any>> = {
  data: (req: any) => new Promise<any>((res) => { const c: any[] = []; req.on("data", (d: any) => c.push(d)); req.on("end", () => res(Buffer.concat(c).length)); }),
  readable: (req: any) => new Promise<any>((res) => { let n = 0; req.on("readable", () => { let ch; while ((ch = req.read()) !== null) n += ch.length; }); req.on("end", () => res(n)); }),
  asyncIter: async (req: any) => { let n = 0; for await (const ch of req) n += ch.length; return n; },
  web: async (req: any) => { const r = Readable.toWeb(req).getReader(); let n = 0; for (;;) { const { done, value } = await r.read(); if (done) break; n += value.byteLength; } return n; },
};
const server = http.createServer(async (req, res) => {
  const mode = req.url.slice(1);
  const n = await Promise.race([readVia[mode](req), new Promise((r) => setTimeout(() => r("timeout"), 3000))]);
  res.end(String(n));
});
server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const m of Object.keys(readVia)) {
    const r = await fetch(`http://127.0.0.1:${port}/${m}`, { method: "POST", body: new Uint8Array(184).fill(7), headers: { "content-type": "image/png" } });
    console.log(m, await r.text());
  }
  server.close();
});
