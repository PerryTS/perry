import * as http from "node:http";
const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (d) => (body += d));
  req.on("end", () => {
    res.setHeader("content-type", "application/json");
    res.end(JSON.stringify({ method: req.method, url: req.url, body }));
  });
});
server.listen(0, async () => {
  const port = (server.address() as any).port;
  const r1 = await fetch("http://127.0.0.1:" + port + "/a?x=1");
  console.log("get", r1.status, r1.headers.get("content-type"), await r1.text());
  const r2 = await fetch("http://127.0.0.1:" + port + "/b", { method: "POST", body: "payload" });
  const j = await r2.json();
  console.log("post", r2.status, j.method, j.url, j.body);
  server.close(() => console.log("closed"));
});
