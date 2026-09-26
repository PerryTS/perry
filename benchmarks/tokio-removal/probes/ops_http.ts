// N sequential http.get round trips against an in-process node:http server.
import * as http from "node:http";
const N = Number(process.argv[2] || 100);
const server = http.createServer((req, res) => res.end("ok"));
function get(port: number): Promise<string> {
  return new Promise((ok, bad) => {
    http.get({ host: "127.0.0.1", port, path: "/", agent: false }, (res) => {
      let b = "";
      res.on("data", (d) => (b += d));
      res.on("end", () => ok(b));
    }).on("error", bad);
  });
}
server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  let n = 0;
  for (let i = 0; i < N; i++) if ((await get(port)) === "ok") n++;
  console.log("http", n);
  server.close();
});
