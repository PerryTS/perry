// Bug 15 (perry-feather#111): a finished client exchange's ClientRequest and
// IncomingMessage give their native handle ids back only through a collection
// that proves nothing names them, and until then they keep answering like
// Node. Every 100th request/response pair is kept and must keep answering
// (status, header, body, res.req, complete), stay distinct from every later
// pair, and no WeakMap entry keyed by a kept request may answer for a later
// one. Every way an exchange ends runs too: response end, destroy before the
// response, destroy mid-body, AbortSignal, timeout + destroy, connection
// refused, upgrade, and a reader that attaches only after 'close' and an
// allocation churn. The id-band pressure run (140k requests) is the
// integration test crates/perry/tests/http_client_handle_ids_released.rs.
// The server speaks raw HTTP/1.1 on `net`, so the only per-request handles are
// the client's.
import http from "node:http";
import net from "node:net";

const TOTAL = 4_000;
const IN_FLIGHT = 64;
const ROUNDS = 200;

function respond(socket: net.Socket, path: string) {
  if (path === "/hang") return;
  if (path === "/slow") {
    socket.write("HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nab");
    return;
  }
  if (path === "/upgrade") {
    socket.write("HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: x\r\n\r\n");
    return;
  }
  const body = "body:" + path;
  socket.write(
    "HTTP/1.1 200 OK\r\nContent-Length: " + body.length + "\r\nx-n: " + path +
      "\r\nConnection: keep-alive\r\n\r\n" + body,
  );
}

const server = net.createServer((socket) => {
  let pending = "";
  socket.setEncoding("latin1");
  socket.on("error", () => {});
  socket.on("data", (chunk: string) => {
    pending += chunk;
    let at: number;
    while ((at = pending.indexOf("\r\n\r\n")) >= 0) {
      const head = pending.slice(0, at);
      pending = pending.slice(at + 4);
      respond(socket, head.split(" ")[1]);
    }
  });
});

function get(port: number, path: string, agent: http.Agent | false, extra: any = {}): Promise<any> {
  return new Promise((resolve) => {
    const out: any = { body: "", error: "", closed: false };
    const req = http.get({ host: "127.0.0.1", port, path, agent, ...extra }, (res) => {
      out.res = res;
      res.setEncoding("latin1");
      res.on("data", (c: string) => (out.body += c));
      res.on("error", () => {});
    });
    out.req = req;
    req.on("error", (e: any) => (out.error = e.code || "error"));
    req.on("close", () => {
      out.closed = true;
      resolve(out);
    });
  });
}

async function main(port: number, refusedPort: number) {
  const t0 = Date.now();
  const lap = (what: string) => console.error(what, Date.now() - t0);
  const agent = new http.Agent({ keepAlive: true, maxSockets: 8 });
  const kept: any[] = [];
  const marks = new WeakMap<object, number>();
  let ok = 0;
  let sameAsKept = 0;
  let weakHits = 0;
  for (let base = 0; base < TOTAL; base += IN_FLIGHT) {
    const batch: Promise<any>[] = [];
    for (let i = base; i < base + IN_FLIGHT && i < TOTAL; i++) batch.push(get(port, "/" + i, agent));
    for (const out of await Promise.all(batch)) {
      if (marks.has(out.req)) weakHits++;
      if (out.res && out.res.statusCode === 200 && out.body === "body:" + out.req.path) ok++;
      const n = Number(out.req.path.slice(1));
      if (n % 100 === 0) {
        for (const k of kept) if (k.req === out.req || k.res === out.res) sameAsKept++;
        kept.push(out);
        marks.set(out.req, n);
      }
    }
  }
  lap("requests");
  console.log("requests ok", ok, "of", TOTAL);
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

  const paths: [string, () => Promise<any>][] = [
    ["destroy before response", () => new Promise((resolve) => {
      const out: any = { error: "" };
      const req = http.get({ host: "127.0.0.1", port, path: "/hang", agent: false });
      req.on("error", (e: any) => (out.error = e.code));
      req.on("close", () => resolve(out));
      req.destroy();
    })],
    ["destroy mid-body", () => new Promise((resolve) => {
      const out: any = { error: "" };
      const req = http.get({ host: "127.0.0.1", port, path: "/slow", agent: false }, (res) => {
        res.on("error", () => {});
        res.on("data", () => req.destroy());
      });
      req.on("error", (e: any) => (out.error = e.code));
      req.on("close", () => resolve(out));
    })],
    ["abort signal", () => {
      const ac = new AbortController();
      const p = get(port, "/hang", false, { signal: ac.signal });
      setTimeout(() => ac.abort(), 0);
      return p;
    }],
    ["timeout then destroy", () => new Promise((resolve) => {
      const out: any = { error: "" };
      const req = http.get({ host: "127.0.0.1", port, path: "/hang", agent: false, timeout: 1 });
      req.on("timeout", () => req.destroy());
      req.on("error", (e: any) => (out.error = e.code));
      req.on("close", () => resolve(out));
    })],
    ["connection refused", () => get(refusedPort, "/x", false)],
  ];
  for (const [name, run] of paths) {
    let closes = 0;
    const errors = new Map<string, number>();
    for (let i = 0; i < ROUNDS; i++) {
      const out = await run();
      closes++;
      errors.set(out.error, (errors.get(out.error) || 0) + 1);
    }
    console.log(name, "closes", closes, "errors", JSON.stringify([...errors].sort()));
  }

  let upgrades = 0;
  for (let i = 0; i < 20; i++) {
    await new Promise<void>((resolve) => {
      const req = http.get({
        host: "127.0.0.1", port, path: "/upgrade", agent: false,
        headers: { Connection: "Upgrade", Upgrade: "x" },
      });
      req.on("upgrade", (res: any, socket: net.Socket) => {
        if (res.statusCode === 101) upgrades++;
        socket.destroy();
        resolve();
      });
    });
  }
  lap("paths");
  console.log("upgrades", upgrades);

  // A reader that attaches only after the exchange finished (Perry's
  // transport reads the whole body and emits 'close'; Node holds it unread)
  // still gets the whole body, across collections.
  const late: any = await new Promise((resolve) => {
    http.get({ host: "127.0.0.1", port, path: "/late", agent }, (res) => resolve(res));
  });
  await new Promise((r) => setTimeout(r, 50));
  let churn: any[] = [];
  for (let i = 0; i < 2_000_000; i++) {
    churn.push({ i });
    if (churn.length > 1000) churn = [];
  }
  let lateBody = "";
  late.setEncoding("latin1");
  late.on("data", (c: string) => (lateBody += c));
  await new Promise((r) => setTimeout(r, 20));
  console.log("late reader", JSON.stringify(lateBody), late.statusCode, late.req.path);
  agent.destroy();
}

const refused = net.createServer();
refused.listen(0, "127.0.0.1", () => {
  const refusedPort = (refused.address() as any).port;
  refused.close(() => {
    server.listen(0, "127.0.0.1", () => {
      main((server.address() as any).port, refusedPort).then(
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
  });
});
