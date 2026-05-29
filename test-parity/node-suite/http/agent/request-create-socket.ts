// Issue #2154 — `http.Agent.createSocket(req, options, cb)` override invoked
// on the request path. PR #2264 shipped the Agent surface and PR #2323 wired
// the `createConnection` override; this pins the sibling `createSocket` path
// (Node's `Agent.prototype.addRequest` semantics): the override is called with
// `(req, options, cb)`, it produces a real `net.connect` socket and delivers
// it via `cb(null, socket)`, and the HTTP exchange flows over that socket back
// to the response handler.
//
// Compared byte-for-byte against `node --experimental-strip-types`.
import http from "node:http";
import net from "node:net";

const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "text/plain" });
  res.end("hello via createSocket");
});

server.listen(0, () => {
  const addr = server.address();
  const port = typeof addr === "object" && addr !== null ? addr.port : 0;

  const agent = new http.Agent();
  let createdSocket = false;
  agent.createSocket = (req: any, options: any, cb: any) => {
    createdSocket = true;
    const sock = net.connect(options.port, options.host);
    cb(null, sock);
  };

  const req = http.request(
    { host: "localhost", port, path: "/", agent },
    (res: any) => {
      let body = "";
      res.on("data", (chunk: any) => {
        body += chunk.toString();
      });
      res.on("end", () => {
        console.log("status:", res.statusCode);
        console.log("body:", body);
        console.log("createSocket called:", createdSocket);
        server.close();
      });
    },
  );
  req.end();
});
