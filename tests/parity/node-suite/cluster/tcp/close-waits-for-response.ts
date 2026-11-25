import cluster from "node:cluster";
import http from "node:http";
import net from "node:net";

// A worker that exits in its `close(callback)` has answered its request first.
if (cluster.isPrimary) {
  const worker = cluster.fork();
  let received = "";
  let exitCode: number | undefined;
  let pending = 2;
  const done = () => {
    if (--pending > 0) return;
    console.log("client got:", received.split("\r\n")[0] || "(nothing)");
    console.log("worker exit:", exitCode);
  };
  worker.on("listening", (address) => {
    const client = net.connect(address.port, "127.0.0.1", () => {
      client.write("GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    });
    client.on("data", (chunk) => (received += chunk.toString()));
    client.on("error", () => {});
    client.on("close", done);
  });
  worker.on("exit", (code) => {
    exitCode = code;
    done();
  });
} else {
  const server = http.createServer((_req, res) => {
    console.log("close called");
    server.close(() => {
      console.log("callback");
      process.exit(0);
    });
    setTimeout(() => {
      console.log("response ends");
      res.end("done");
    }, 100);
  });
  server.listen(0, "127.0.0.1");
}
