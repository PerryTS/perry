// Issue #2154 — `http.Agent.createConnection` override invoked on the
// request path. PR #2264 shipped the Agent surface (validation, pool
// accessors, per-agent reqwest client, setter storage) but explicitly
// deferred *invoking* a user-supplied `createConnection` when servicing
// `http.request`. This test pins the full socket-injection behavior: the
// override is called, it produces a real `net.connect` socket, and the
// HTTP exchange flows over that socket back to the response handler.
//
// Compared byte-for-byte against `node --experimental-strip-types`.
import http from "node:http";
import net from "node:net";

const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "text/plain" });
  res.end("hello from server");
});

server.listen(0, () => {
  const addr = server.address();
  const port = typeof addr === "object" && addr !== null ? addr.port : 0;

  const agent = new http.Agent();
  let created = false;
  agent.createConnection = (options: any) => {
    created = true;
    return net.connect(options.port, options.host);
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
        console.log("createConnection called:", created);
        server.close();
      });
    },
  );
  req.end();
});
