// Bug 9: a server's per-request objects are native handles whose ids return to
// a shared pool. Requests on one keep-alive connection used to get the id of
// the previous, finished request, so `req` compared `===` to an old request
// still held by the program, and a WeakMap keyed by the old request answered
// for the new one. An id may be reissued only after the collector proves that
// no value, and no weak entry, still names it.
import http from "node:http";

const byReq = new WeakMap<object, number>();
const byRes = new WeakMap<object, number>();
let held: any[] = [];
let resHeld: any[] = [];
let n = 0;
const errors: string[] = [];

function fail(msg: string) {
  if (errors.length < 5) errors.push(msg);
}

const server = http.createServer((req: any, res: any) => {
  const i = n++;
  if (byReq.get(req) !== undefined) fail(`req ${i}: WeakMap hit ${byReq.get(req)}`);
  if (byRes.get(res) !== undefined) fail(`res ${i}: WeakMap hit ${byRes.get(res)}`);
  if (req.tag !== undefined) fail(`req ${i}: stale expando ${req.tag}`);
  if (res.locals !== undefined) fail(`res ${i}: stale locals`);
  for (let j = 0; j < held.length; j++) {
    if (held[j] === req) fail(`req ${i} === held req ${j}`);
    if (resHeld[j] === res) fail(`res ${i} === held res ${j}`);
  }
  byReq.set(req, i);
  byRes.set(res, i);
  req.tag = i;
  res.locals = { i };
  held.push(req);
  resHeld.push(res);
  res.end("ok " + i);
});

const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });

function get(port: number, path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    http
      .get({ host: "127.0.0.1", port, path, agent }, (res) => {
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (c: string) => (body += c));
        res.on("end", () => resolve(body));
      })
      .on("error", reject);
  });
}

function checkHeld(phase: string) {
  let wrong = 0;
  for (let j = 0; j < held.length; j++) {
    const k = byReq.get(held[j]);
    if (k !== j + (phase === "phase 1" ? 0 : 50) || byRes.get(resHeld[j]) !== k) wrong++;
  }
  console.log(phase, "held", held.length, "wrong", wrong);
}

function churn() {
  let junk: any[] = [];
  for (let i = 0; i < 200000; i++) junk.push({ i });
  junk = [];
  (globalThis as any).gc?.();
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  let bodies = 0;
  for (let i = 0; i < 25; i++) if ((await get(port, "/agent/" + i)).startsWith("ok ")) bodies++;
  for (let i = 0; i < 25; i++) {
    const r = await fetch(`http://127.0.0.1:${port}/fetch/${i}`);
    if ((await r.text()).startsWith("ok ")) bodies++;
  }
  checkHeld("phase 1");
  held = [];
  resHeld = [];
  churn();
  for (let i = 0; i < 25; i++) if ((await get(port, "/after/" + i)).startsWith("ok ")) bodies++;
  for (let i = 0; i < 25; i++) {
    const r = await fetch(`http://127.0.0.1:${port}/after-fetch/${i}`);
    if ((await r.text()).startsWith("ok ")) bodies++;
  }
  checkHeld("phase 2");
  console.log("requests", n, "bodies", bodies);
  console.log("errors", errors.length === 0 ? "none" : errors.join("; "));
  process.exit(0);
});
