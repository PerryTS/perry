// NXPB2-P2: a live server's ServerResponse is born on its heap layout and
// waits on the server while the handler awaits. Allocation churn between
// arrival, the await and res.end() moves young objects (run it under
// PERRY_GC_FORCE_EVACUATE=1); the parked response must keep its identity.
import * as http from "node:http";

const churn: unknown[] = [];
function allocate(n: number): void {
  for (let i = 0; i < n; i++) churn.push({ i, s: "x" + i, a: [i, i + 1] });
  if (churn.length > 20000) churn.length = 0;
}

const server = http.createServer(async (req, res) => {
  allocate(4000);
  await new Promise((resolve) => setTimeout(resolve, 20));
  allocate(4000);
  const sameReq = res.req === req;
  const isResponse = res instanceof http.ServerResponse;
  const keys = Object.keys(res).join(",");
  res.setHeader("x-path", req.url ?? "");
  res.end(JSON.stringify({ url: req.url, sameReq, isResponse, keys }));
});

function get(port: number, path: string): Promise<string> {
  return new Promise((resolve, reject) => {
    http
      .get({ host: "127.0.0.1", port, path }, (res) => {
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (chunk) => (body += chunk));
        res.on("end", () => resolve(`${res.statusCode} ${res.headers["x-path"]} ${body}`));
      })
      .on("error", reject);
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as { port: number }).port;
  const bodies = await Promise.all(["/a", "/b", "/c"].map((path) => get(port, path)));
  for (const body of bodies) console.log(body);
  server.close(() => console.log("closed"));
});
