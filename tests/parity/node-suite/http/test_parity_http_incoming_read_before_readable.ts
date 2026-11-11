// A paused-mode reader that calls read() before the first "readable" event
// (effect platform-node readableToPull) still sees the body and then "end".
import * as http from "node:http";
// effect's readableToPull: listeners first, then read() right away, waiting for 'readable'/'end' when null.
const pullAll = (req: any) => new Promise<any>((resolve) => {
  let ended = false; let wake: null | (() => void) = null; let total = 0;
  req.on("readable", () => wake && wake());
  req.once("end", () => { ended = true; wake && wake(); });
  const loop = () => {
    const item = req.read();
    if (item === null) {
      if (ended || req.readableEnded) return resolve(total);
      wake = () => { wake = null; loop(); };
      return;
    }
    total += item.length;
    let more; while ((more = req.read()) !== null) total += more.length;
    queueMicrotask(loop);
  };
  loop();
});
const server = http.createServer(async (req, res) => {
  const n = await Promise.race([pullAll(req), new Promise<any>((r) => setTimeout(() => r("timeout"), 3000))]);
  res.statusCode = 204; res.setHeader("x-bytes", String(n)); res.end();
});
server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const size of [184, 1117, 0]) {
    const r = await fetch(`http://127.0.0.1:${port}/u`, { method: "POST", body: new Uint8Array(size).fill(1) });
    console.log(size, r.status, r.headers.get("x-bytes"));
  }
  server.close();
});
