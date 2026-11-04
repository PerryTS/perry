// The head `res.end()` sends is the head the response committed, whichever
// way it was committed: implicitly by `setHeader` + `end`, by `writeHead` with
// a header object (array values included), by `writeHead` after `setHeader`,
// and by `flushHeaders` before `end`. After the end the response still answers
// `getHeader` / `getHeaders` / `headersSent`, and a second `end()` changes
// nothing on the wire (perry-feather#111 bug 15: the end takes the committed
// head instead of copying it).
import http from "node:http";

const server = http.createServer((req, res) => {
  switch (req.url) {
    case "/implicit":
      res.setHeader("X-A", "1");
      res.setHeader("Content-Length", "1");
      res.end("k");
      break;
    case "/object":
      res.writeHead(201, { "X-B": "2", "Set-Cookie": ["a=1", "b=2"] });
      res.end("ok");
      break;
    case "/merged":
      res.setHeader("X-C", "3");
      res.writeHead(202, { "X-D": "4" });
      res.end("merged");
      break;
    case "/flushed":
      res.setHeader("X-E", "5");
      res.flushHeaders();
      res.end("flushed");
      break;
  }
  const again = res.end();
  console.log(
    req.url,
    again === res,
    res.headersSent,
    res.writableEnded,
    String(res.getHeader("x-a") ?? res.getHeader("x-c") ?? res.getHeader("x-e") ?? "-"),
    JSON.stringify(Object.keys(res.getHeaders()).sort()),
  );
});

function get(port: number, path: string): Promise<string> {
  return new Promise((resolve) => {
    http
      .get({ host: "127.0.0.1", port, path, agent: false }, (res) => {
        let body = "";
        res.setEncoding("utf8");
        res.on("data", (c) => (body += c));
        res.on("end", () => {
          const h = res.headers;
          const own = Object.keys(h)
            .filter((k) => k.startsWith("x-") || k === "set-cookie" || k === "content-length")
            .sort()
            .map((k) => k + "=" + JSON.stringify(h[k]));
          resolve([path, res.statusCode, body, ...own].join(" "));
        });
      })
      .on("error", (e) => resolve(path + " error " + (e as any).code));
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const path of ["/implicit", "/object", "/merged", "/flushed"]) console.log(await get(port, path));
  server.close();
});
