import * as net from "node:net";

// #2013: server.listen(port) rejects a numeric port outside the integer range
// [0, 65536) with RangeError [ERR_SOCKET_BAD_PORT], matching Node. A string
// first argument is a pipe path and is NOT range-checked.
const badPorts: any[] = [-1, 65536, 100000, NaN, 3.5, Infinity, -Infinity];

for (const port of badPorts) {
  try {
    const server = net.createServer();
    server.listen(port);
    console.log("listen", String(port), "=> NO THROW");
  } catch (err: any) {
    console.log("listen", String(port), "=>", err.name, err.code, "|", err.message);
  }
}

// createServer() returns a Server object without performing any I/O.
console.log("createServer typeof:", typeof net.createServer());
