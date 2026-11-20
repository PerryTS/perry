// Regression: `new Request(url, { body: req })` where `req` is a Node
// `http.IncomingMessage` must read the request's buffered body.
//
// Next.js's App Router bridges a Node request into the Web platform via
// `NextRequestAdapter.fromNodeNextRequest`, which hands the raw `IncomingMessage`
// straight to `new Request(url, { body: req, duplex: 'half' })` and then reads it
// back with `request.text()` / `request.formData()`. Perry's Request constructor
// only extracted bytes from Buffer / typed-array / Blob / string bodies, so a
// Node `IncomingMessage` body (a small native handle) was dropped and the Web
// Request came back empty — which made Auth.js reject every credentials login
// with `MissingCSRF`. The Response constructor already had this reader (#5437);
// the Request path never did. #6432.
//
// The observable is the body length read back through `request.text()`,
// byte-for-byte identical to `node --experimental-strip-types`.

import { createServer, request } from "node:http";

const PORT = 18994;
const BODY = "csrfToken=abc123&email=user@example.com&password=secret";

const server = createServer(async (req: any, res: any) => {
  let out = "";
  try {
    const webReq = new Request("http://x/", {
      method: "POST",
      body: req,
      duplex: "half",
      headers: { "content-type": "application/x-www-form-urlencoded" },
    } as any);
    const txt = await webReq.text();
    out = `len=${txt.length} match=${txt === BODY}`;
  } catch (e: any) {
    out = `threw=${String((e && e.message) || e)}`;
  }
  res.statusCode = 200;
  res.end(out);
});

server.listen(PORT, () => {
  const r = request(
    { host: "localhost", port: PORT, path: "/", method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded", "Content-Length": Buffer.byteLength(BODY) } },
    (res: any) => {
      let o = ""; res.setEncoding("utf8");
      res.on("data", (c: string) => (o += c));
      res.on("end", () => { console.log(o); console.log(`expected len=${Buffer.byteLength(BODY)}`); server.close(); });
    }
  );
  r.write(BODY); r.end();
});
