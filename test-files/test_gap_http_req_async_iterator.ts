// Regression: an http `IncomingMessage` (the request) must be async-iterable, so
// `for await (const chunk of req)` reads the request body.
//
// Node's `http.IncomingMessage` is a Readable stream with `[Symbol.asyncIterator]`.
// Perry represents the request as a native handle; it used to expose no async
// iterator, so `for await…of req` threw `is not iterable` and yielded 0 bytes.
// That broke every framework that reads the POST body via async iteration — e.g.
// Next.js's `requestToBodyStream` does exactly `for await (const chunk of stream)`,
// so an App Router POST (and Auth.js's CSRF check) saw an empty body. #6432.
//
// The observable here is the byte count read back through `for await…of req`,
// byte-for-byte identical to `node --experimental-strip-types`.

import { createServer, request } from "node:http";

const PORT = 18992;
const BODY = "csrfToken=abc123&email=user@example.com&password=secret&remember=1";

const server = createServer(async (req: any, res: any) => {
  let bytes = 0;
  let threw = "";
  try {
    for await (const chunk of req) {
      bytes += chunk.length;
    }
  } catch (e: any) {
    threw = String((e && e.message) || e);
  }
  res.statusCode = 200;
  res.setHeader("Content-Type", "text/plain");
  res.end(`bytes=${bytes} threw=${threw}`);
});

server.listen(PORT, () => {
  const req = request(
    {
      host: "localhost",
      port: PORT,
      path: "/",
      method: "POST",
      headers: {
        "Content-Type": "application/x-www-form-urlencoded",
        "Content-Length": Buffer.byteLength(BODY),
      },
    },
    (res: any) => {
      let out = "";
      res.setEncoding("utf8");
      res.on("data", (c: string) => (out += c));
      res.on("end", () => {
        console.log(out);
        console.log(`expected bytes=${Buffer.byteLength(BODY)}`);
        server.close();
      });
    }
  );
  req.write(BODY);
  req.end();
});
