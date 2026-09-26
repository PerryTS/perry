// N sequential fetch() round trips against an in-process node:http server.
import * as http from "node:http";
const N = Number(process.argv[2] || 100);
const server = http.createServer((req, res) => res.end("ok"));
server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  let n = 0;
  for (let i = 0; i < N; i++) if ((await (await fetch("http://127.0.0.1:" + port + "/")).text()) === "ok") n++;
  console.log("fetch", n);
  server.close();
});
