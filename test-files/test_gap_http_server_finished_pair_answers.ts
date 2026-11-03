// A finished server request/response pair that JS keeps answers like Node
// (perry-feather#111, bug 9 open question 2): url, method, headers, an
// expando, `res.req`, status and headers stay readable after the response
// ended and after collections, for pairs finished by a synchronous handler and
// by a handler that ends later. A handler that writes after its client went
// away (its request saw 'aborted') completes. Kept pairs stay distinct from every later pair.
import http from "node:http";

const TOTAL = 3000;
const IN_FLIGHT = 32;

const kept: any[] = [];
let late: Promise<string> | undefined;
let lateRes: any;

const server = http.createServer((req, res) => {
  const n = Number(req.url!.slice(1)) || 0;
  (req as any).tag = "t" + n;
  res.setHeader("x-a", "a" + n);
  if (req.url === "/gone") {
    lateRes = res;
    late = new Promise((resolve) => {
      req.on("aborted", () =>
        setTimeout(() => {
          res.write("late");
          res.end();
          resolve("late write done");
        }, 20),
      );
    });
    return;
  }
  if (n % 100 === 0) kept.push({ n, req, res });
  if (n % 2 === 0) {
    res.end("ok");
  } else {
    setTimeout(() => res.end("ok"), 0);
  }
});

function get(port: number, path: string, agent: http.Agent): Promise<number> {
  return new Promise((resolve) => {
    http
      .get({ host: "127.0.0.1", port, path, agent }, (res) => {
        res.resume();
        res.on("end", () => resolve(res.statusCode!));
      })
      .on("error", () => resolve(0));
  });
}

async function main(port: number) {
  const agent = new http.Agent({ keepAlive: true, maxSockets: 8 });
  let ok = 0;
  for (let base = 0; base < TOTAL; base += IN_FLIGHT) {
    const batch: Promise<number>[] = [];
    for (let i = base; i < base + IN_FLIGHT && i < TOTAL; i++) batch.push(get(port, "/" + i, agent));
    for (const status of await Promise.all(batch)) if (status === 200) ok++;
  }
  let churn: any[] = [];
  for (let i = 0; i < 2_000_000; i++) {
    churn.push({ i });
    if (churn.length > 1000) churn = [];
  }
  await new Promise((r) => setTimeout(r, 20));
  let answers = 0;
  let distinct = 0;
  const seen = new Set<any>();
  for (const k of kept) {
    if (!seen.has(k.req) && !seen.has(k.res)) distinct++;
    seen.add(k.req);
    seen.add(k.res);
    if (
      k.req.url === "/" + k.n && k.req.method === "GET" && k.req.headers.host === "127.0.0.1:" + port &&
      k.req.tag === "t" + k.n && k.res.req === k.req && k.res.statusCode === 200 &&
      k.res.getHeader("x-a") === "a" + k.n && k.res.headersSent && k.res.writableEnded &&
      k.res.writableFinished && k.req.complete
    ) answers++;
  }
  console.log("requests ok", ok, "of", TOTAL);
  console.log("kept", kept.length, "distinct", distinct, "answering like node", answers);

  await new Promise<void>((resolve) => {
    const req = http.get({ host: "127.0.0.1", port, path: "/gone", agent: false });
    req.on("error", () => {});
    req.on("close", () => resolve());
    setTimeout(() => req.destroy(), 20);
  });
  console.log(await late!, lateRes.writableEnded, lateRes.req.url);
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
