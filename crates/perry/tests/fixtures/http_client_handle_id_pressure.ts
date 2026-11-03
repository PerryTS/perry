// Bug 15 (perry-feather#111): every http.get registers two native handle ids
// (the ClientRequest and its IncomingMessage) in a shared band of 262k, so
// 140k requests exhaust it unless finished exchanges give their ids back. They
// may come back only through a collection that proves nothing names them:
// every 1000th request/response pair is kept and must keep answering like Node
// (status, header, body, res.req, complete), stay distinct from every later
// pair, and no WeakMap entry keyed by a kept request may answer for a later
// one. The server speaks raw HTTP/1.1 on `net`, so the only per-request handles
// are the client's.
import http from "node:http";
import net from "node:net";

const TOTAL = 140_000;
const IN_FLIGHT = 64;

const server = net.createServer((socket) => {
  let pending = "";
  socket.setEncoding("latin1");
  socket.on("error", () => {});
  socket.on("data", (chunk: string) => {
    pending += chunk;
    let at: number;
    while ((at = pending.indexOf("\r\n\r\n")) >= 0) {
      const path = pending.slice(0, at).split(" ")[1];
      pending = pending.slice(at + 4);
      const body = "body:" + path;
      socket.write(
        "HTTP/1.1 200 OK\r\nContent-Length: " + body.length + "\r\nx-n: " + path +
          "\r\nConnection: keep-alive\r\n\r\n" + body,
      );
    }
  });
});

function get(port: number, path: string, agent: http.Agent): Promise<any> {
  return new Promise((resolve) => {
    const out: any = { body: "" };
    const req = http.get({ host: "127.0.0.1", port, path, agent }, (res) => {
      out.res = res;
      res.setEncoding("latin1");
      res.on("data", (c: string) => (out.body += c));
    });
    out.req = req;
    req.on("error", (e: any) => (out.error = e.code || "error"));
    req.on("close", () => resolve(out));
  });
}

async function main(port: number) {
  const agent = new http.Agent({ keepAlive: true, maxSockets: 8 });
  const kept: any[] = [];
  const marks = new WeakMap<object, number>();
  let ok = 0;
  let sameAsKept = 0;
  let weakHits = 0;
  const errors = new Set<string>();
  for (let base = 0; base < TOTAL; base += IN_FLIGHT) {
    const batch: Promise<any>[] = [];
    for (let i = base; i < base + IN_FLIGHT && i < TOTAL; i++) batch.push(get(port, "/" + i, agent));
    for (const out of await Promise.all(batch)) {
      if (out.error) errors.add(out.error);
      if (marks.has(out.req)) weakHits++;
      if (out.res && out.res.statusCode === 200 && out.body === "body:" + out.req.path) ok++;
      const n = Number(out.req.path.slice(1));
      if (n % 1000 === 0) {
        for (const k of kept) if (k.req === out.req || k.res === out.res) sameAsKept++;
        kept.push(out);
        marks.set(out.req, n);
      }
    }
  }
  console.log("requests ok", ok, "of", TOTAL, "errors", JSON.stringify([...errors].sort()));
  console.log("kept", kept.length, "same as a later one", sameAsKept, "weak hits", weakHits);
  let answers = 0;
  for (const k of kept) {
    const n = k.req.path;
    if (
      k.res.statusCode === 200 && k.res.headers["x-n"] === n && k.res.req === k.req &&
      k.res.complete && k.body === "body:" + n && marks.get(k.req) === Number(n.slice(1))
    ) answers++;
  }
  console.log("kept pairs answering like node", answers);
  agent.destroy();
}

server.listen(0, "127.0.0.1", () => {
  main((server.address() as any).port).then(
    () => {
      server.close();
      process.exit(0);
    },
    (e) => {
      console.log("failed", e && (e.code || e.message));
      process.exit(1);
    },
  );
});
