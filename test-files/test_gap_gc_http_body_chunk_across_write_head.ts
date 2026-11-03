import * as http from "node:http";

// A program-replaced `writeHead` (what on-headers does for `compression`)
// runs inside the implicit head of `res.end(chunk)` / `res.write(chunk, cb)`.
// It allocates enough garbage that a collection runs there; the chunk the
// response was handed is a young string and must still be the body.
function churn(): number {
  let keep = 0;
  for (let i = 0; i < 400000; i++) {
    const o = { i, s: "v" + i, a: [i, i + 1] };
    keep += o.a.length;
  }
  return keep;
}

function payload(tag: string, n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) s += "<p>" + tag + ":" + i + "</p>";
  return s;
}

const server = http.createServer((req, res) => {
  const original = res.writeHead;
  res.writeHead = function (this: http.ServerResponse, ...args: any[]) {
    churn();
    return (original as any).apply(this, args);
  } as any;
  const url = req.url || "/";
  const body = payload(url, 400);
  res.setHeader("X-Expected-Length", String(Buffer.byteLength(body)));
  if (url.startsWith("/end")) {
    res.end(body);
  } else {
    res.write(body, () => {});
    res.end();
  }
});

function get(port: number, path: string): Promise<string> {
  return new Promise((resolve) => {
    http.get({ host: "127.0.0.1", port, path }, (res) => {
      let n = 0;
      let text = "";
      res.on("data", (d: Buffer) => { n += d.length; text += d.toString(); });
      res.on("end", () => {
        const expected = Number(res.headers["x-expected-length"]);
        const ok = n === expected && text === payload(path, 400);
        resolve(path + " status=" + res.statusCode + " bytes=" + n + " expected=" + expected + " same=" + ok);
      });
    });
  });
}

server.listen(0, "127.0.0.1", async () => {
  const port = (server.address() as any).port;
  for (const path of ["/end1", "/end2", "/end3", "/write1", "/write2", "/write3"]) {
    console.log(await get(port, path));
  }
  server.close();
});
