// perry-test: run-timeout=60
// A finished request whose own properties name it (Express's `req.res = res`,
// whose `res.req` is the request, and a direct `req.self = req`) is still
// reclaimed (#15172): its property bag is traced only when a reachable value
// names the request. 300k requests retire more than the 262k-id band, so a
// request kept alive by its own bag exhausts it.
// The client writes raw keep-alive HTTP/1.1 on `net` sockets, so the only
// per-request handles are the server's.
import http from "node:http";
import net from "node:net";

const TOTAL = 300_000;
const CONNECTIONS = 4;
const kept: any[] = [];
const marks = new WeakMap<object, number>();
let served = 0;
let sameAsKept = 0;
let weakHits = 0;

const server = http.createServer((req: any, res: any) => {
  const i = served++;
  if (marks.has(req)) weakHits++;
  if (i % 1000 === 0) {
    for (const old of kept) if (old === req) sameAsKept++;
    kept.push(req);
    marks.set(req, i);
  }
  req.res = res;
  req.self = req;
  res.setHeader("Content-Length", "1");
  res.end("k");
});

const REQUEST = "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n";
const END = "\r\n\r\nk";

function connection(port: number, count: number): Promise<number> {
  return new Promise((resolve, reject) => {
    let answered = 0;
    let pending = "";
    const socket = net.connect(port, "127.0.0.1", () => socket.write(REQUEST));
    socket.setEncoding("latin1");
    socket.on("data", (chunk: string) => {
      pending += chunk;
      let at: number;
      while ((at = pending.indexOf(END)) >= 0) {
        pending = pending.slice(at + END.length);
        answered++;
        if (answered === count) {
          socket.end();
          resolve(answered);
          return;
        }
        socket.write(REQUEST);
      }
    });
    socket.on("error", reject);
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  let answered = 0;
  let failed = 0;
  const loops: Promise<void>[] = [];
  for (let c = 0; c < CONNECTIONS; c++) {
    loops.push(
      connection(port, TOTAL / CONNECTIONS).then(
        (n) => {
          answered += n;
        },
        (e: any) => {
          failed++;
          if (failed === 1) console.log("first error", e.code ?? e.message);
        },
      ),
    );
  }
  await Promise.all(loops);
  let keptOk = 0;
  for (let j = 0; j < kept.length; j++) if (kept[j] === kept[j] && marks.get(kept[j]) === j * 1000) keptOk++;
  console.log("served", served, "answered", answered, "failed", failed);
  console.log("kept", kept.length, "intact", keptOk, "same as kept", sameAsKept, "weak hits", weakHits);
  process.exit(0);
});
