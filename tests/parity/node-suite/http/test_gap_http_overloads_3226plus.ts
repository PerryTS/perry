// Gap test: node:http client factory argument-overload normalization
// (#3226 / #3227 / #3228). Byte-for-byte parity against
// `node --experimental-strip-types`.
//
// Exercises every Node overload of http.request / http.get against a
// LOCAL ephemeral-port server (no external network):
//   - request(url, cb)
//   - request(options, cb)
//   - request(url, options, cb)   ← the form that previously dropped the
//                                    callback / mis-routed the options object
//   - get(url, cb)
//   - get(options, cb)
//   - get(url, options, cb)
//
// Each handler echoes the request method + url so the response proves the
// URL and options merged correctly (e.g. `request(url, {method:"PUT"})`
// must yield `PUT /c`, while `get(url, {...})` keeps `GET`). Requests run
// sequentially via callback chaining so output is deterministic; the
// server closes after the last response so the program exits.

import * as http from "node:http";

const server = http.createServer((req, res) => {
  res.writeHead(200, { "content-type": "text/plain" });
  res.end(`${req.method} ${req.url}`);
});

server.listen(0, "127.0.0.1", () => {
  const addr: any = server.address();
  const port = addr.port;
  const base = `http://127.0.0.1:${port}`;
  const lines: string[] = [];

  function finish() {
    for (const l of lines) console.log(l);
    server.close();
  }

  // get(url, options, cb) — get() keeps method GET even with options.
  function step6() {
    const r = http.get(`${base}/f`, { headers: { "x-test": "1" } }, (res: any) => {
      let body = "";
      res.on("data", (c: any) => {
        body += c.toString();
      });
      res.on("end", () => {
        lines.push(`get(url,options,cb): status=${res.statusCode} body=${body}`);
        finish();
      });
    });
    r.on("error", () => finish());
  }

  // get(options, cb)
  function step5() {
    const r = http.get({ host: "127.0.0.1", port, path: "/e" }, (res: any) => {
      let body = "";
      res.on("data", (c: any) => {
        body += c.toString();
      });
      res.on("end", () => {
        lines.push(`get(options,cb): status=${res.statusCode} body=${body}`);
        step6();
      });
    });
    r.on("error", () => step6());
  }

  // get(url, cb)
  function step4() {
    const r = http.get(`${base}/d`, (res: any) => {
      let body = "";
      res.on("data", (c: any) => {
        body += c.toString();
      });
      res.on("end", () => {
        lines.push(`get(url,cb): status=${res.statusCode} body=${body}`);
        step5();
      });
    });
    r.on("error", () => step5());
  }

  // request(url, options, cb) — URL gives host/path, options give method.
  function step3() {
    const r = http.request(`${base}/c`, { method: "PUT" }, (res: any) => {
      let body = "";
      res.on("data", (c: any) => {
        body += c.toString();
      });
      res.on("end", () => {
        lines.push(`request(url,options,cb): status=${res.statusCode} body=${body}`);
        step4();
      });
    });
    r.on("error", () => step4());
    r.end();
  }

  // request(options, cb)
  function step2() {
    const r = http.request(
      { host: "127.0.0.1", port, path: "/b", method: "POST" },
      (res: any) => {
        let body = "";
        res.on("data", (c: any) => {
          body += c.toString();
        });
        res.on("end", () => {
          lines.push(`request(options,cb): status=${res.statusCode} body=${body}`);
          step3();
        });
      },
    );
    r.on("error", () => step3());
    r.end();
  }

  // request(url, cb)
  const r = http.request(`${base}/a`, (res: any) => {
    let body = "";
    res.on("data", (c: any) => {
      body += c.toString();
    });
    res.on("end", () => {
      lines.push(`request(url,cb): status=${res.statusCode} body=${body}`);
      step2();
    });
  });
  r.on("error", () => step2());
  r.end();
});
