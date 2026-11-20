// #11586: `server.close()` (Node 19+) destroys only IDLE keep-alive sockets.
// A connection that has sent part of a request head is active, not idle, and
// must stay open until `closeAllConnections()`. The turnloop port counted it
// as idle (it tracked only decoded heads and in-flight responses), so the
// half-sent client was destroyed by close(). Plain-HTTP twin of
// test_issue_4971_tls_connect_options.
import { createServer } from "node:http";
import { connect } from "node:net";

let halfClosed = false;
let idleClosed = false;

const server = createServer((_req, res) => {
  res.writeHead(200, { Connection: "keep-alive" });
  res.end("ok");
});

server.listen(0, () => {
  const port = (server.address() as any).port;
  const half = connect({ port, host: "127.0.0.1" });
  half.on("close", () => { halfClosed = true; });
  half.on("error", () => {});
  half.on("connect", () => {
    half.write("GET / HTTP/1.1"); // no CRLF: the head is incomplete
    const idle = connect({ port, host: "127.0.0.1" });
    idle.on("close", () => { idleClosed = true; });
    idle.on("error", () => {});
    let response = "";
    idle.on("data", (chunk: any) => {
      response += chunk.toString("utf8");
      if (response.indexOf("ok") < 0) return;
      console.log("idle got response:", response.indexOf("200 OK") >= 0);
      server.close();
      setTimeout(() => {
        console.log("after close: half-sent closed =", halfClosed);
        console.log("after close: idle closed =", idleClosed);
        server.closeAllConnections();
        setTimeout(() => {
          console.log("after closeAll: half-sent closed =", halfClosed);
          process.exit(0);
        }, 300);
      }, 300);
    });
    idle.write("GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
  });
});
