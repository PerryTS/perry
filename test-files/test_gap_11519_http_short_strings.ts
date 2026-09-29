// #11519: node:http surfaces given a SHORT (SSO, <= 5 bytes, built at runtime)
// string: event names (`req.on("da" + "ta")`), a response body written with
// `res.end(String(n))`, header names/values, a request method and path. The
// http natives unboxed these with a bare mask or accepted the heap tag only,
// so listeners never registered and the exchange hung or lost its body.
import * as http from "node:http";
const S = (s: string): string => s.charAt(0) + s.slice(1);

const srv = http.createServer((req, res) => {
  let body = "";
  req.on(S("data"), (c: any) => {
    body += c;
  });
  req.on(S("end"), () => {
    res.setHeader(S("X-A"), S("1"));
    res.writeHead(200, { [S("X-C")]: String(3), "Content-Length": S("3") });
    res.write(S("w:"));
    res.end(String(body.length));
  });
});
srv.listen(0, () => {
  const port = (srv.address() as any).port;
  const req = http.request({ port, method: S("POST"), path: S("/p") }, (res) => {
    let d = "";
    res.setEncoding(S("utf8") as any);
    res.on(S("data"), (c: any) => (d += c));
    res.on(S("end"), () => {
      console.log(res.statusCode, res.headers["x-a"], res.headers["x-c"], res.headers["content-length"], JSON.stringify(d));
      srv.close();
    });
  });
  req.setHeader(S("X-B"), S("2"));
  req.write(S("abc"));
  req.end(S("de"));
});
