// #11452: every completed `http.get` round trip against an in-process server
// used to leave one socket descriptor open, so a long-running program ran out
// of descriptors (`connect ECONNREFUSED` after ~1,015 requests at the default
// 1024 limit). The server half-closed each connection once its response was
// written and then never closed it when the client's FIN arrived.
//
// `agent: false` is Node's throwaway `keepAlive: false` agent, so the request
// also says `Connection: close`; perry used to send `keep-alive` there.
//
// Deterministic facts only: every accepted server socket emits 'close', and
// the process's open-descriptor count stays bounded.
import * as http from "node:http";
import * as fs from "node:fs";

function openFds(): number {
  for (const dir of ["/proc/self/fd", "/dev/fd"]) {
    try {
      return fs.readdirSync(dir).length;
    } catch {}
  }
  return -1;
}

let accepted = 0;
let closed = 0;
const connectionHeaders = new Set<string>();
const server = http.createServer((req, res) => {
  connectionHeaders.add(String(req.headers.connection));
  res.end("ok");
});
server.on("connection", (socket) => {
  accepted++;
  socket.on("close", () => closed++);
});

function get(port: number, agent: false | undefined): Promise<string> {
  return new Promise((ok, bad) => {
    const opts: any = { host: "127.0.0.1", port, path: "/" };
    if (agent === false) opts.agent = false;
    http
      .get(opts, (res) => {
        let b = "";
        res.on("data", (d) => (b += d));
        res.on("end", () => ok(b));
      })
      .on("error", bad);
  });
}

function settle(): Promise<void> {
  return new Promise((r) => setTimeout(r, 200));
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  const before = openFds();
  const N = 1200;
  let ok = 0;
  for (let i = 0; i < N; i++) if ((await get(port, false)) === "ok") ok++;
  await settle();
  const after = openFds();
  console.log("agent:false completed", ok === N);
  console.log("agent:false every server socket closed", accepted > 0 && closed === accepted);
  console.log("agent:false fds bounded", before < 0 || after - before < 16);
  console.log("agent:false connection header", [...connectionHeaders].join(","));
  connectionHeaders.clear();

  // The implicit global agent (keep-alive in Node >= 19): still bounded.
  let ok2 = 0;
  for (let i = 0; i < 300; i++) if ((await get(port, undefined)) === "ok") ok2++;
  await settle();
  const after2 = openFds();
  console.log("global agent completed", ok2 === 300);
  console.log("global agent fds bounded", before < 0 || after2 - before < 16);
  console.log("global agent connection header", [...connectionHeaders].join(","));
  server.close(() => console.log("server closed"));
  server.closeIdleConnections?.();
});
