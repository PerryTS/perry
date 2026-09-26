// Mixed "typical backend": http server + fetch + crypto + zlib + timers.
// Modes: (default) self-test and exit; "serve <port>" run until killed;
// "startup" bind an ephemeral port, close, exit (startup-time probe).
import * as http from "node:http";
import * as crypto from "node:crypto";
import * as zlib from "node:zlib";

const payload = JSON.stringify({ items: Array.from({ length: 32 }, (_, i) => ({ id: i, name: "item-" + i })) });

function handle(req: http.IncomingMessage, res: http.ServerResponse) {
  const h = crypto.createHash("sha256").update(req.url || "/").digest("hex");
  zlib.gzip(Buffer.from(payload), (err, gz) => {
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ hash: h.slice(0, 16), gz: err ? -1 : gz.length > 0 }));
  });
}

const mode = process.argv[2] || "selftest";
const server = http.createServer(handle);

if (mode === "serve") {
  const port = Number(process.argv[3] || 18080);
  server.listen(port, "127.0.0.1", () => console.log("listening " + port));
} else if (mode === "startup") {
  server.listen(0, "127.0.0.1", () => server.close(() => console.log("startup ok")));
} else {
  server.listen(0, "127.0.0.1", async () => {
    const port = (server.address() as any).port;
    const r = await fetch("http://127.0.0.1:" + port + "/a");
    const j = await r.json();
    console.log("fetch", r.status, j.hash, j.gz);
    const key = await new Promise<Buffer>((ok, bad) =>
      crypto.pbkdf2("pw", "salt", 1000, 16, "sha256", (e, k) => (e ? bad(e) : ok(k))),
    );
    console.log("pbkdf2", key.toString("hex"));
    await new Promise((ok) => setTimeout(ok, 10));
    let ticks = 0;
    await new Promise<void>((ok) => {
      const iv = setInterval(() => {
        if (++ticks === 3) { clearInterval(iv); ok(); }
      }, 2);
    });
    console.log("ticks", ticks);
    server.close(() => console.log("closed"));
  });
}
