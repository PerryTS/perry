// `const net = require('net'); net.connect(port, host, cb)` — the socket factory
// reached as a bound VALUE through the CJS externals wrapper, which is how mysql2
// (bundled by turbopack) opens its connection. The call arrives at the runtime's
// native-module dispatch rather than the static codegen table, and `net.connect` /
// `net.createConnection` had no arm there: they read as non-callable, so the
// connection was never opened.

import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const net = require("net");

console.log("connect          :", typeof net.connect);
console.log("createConnection :", typeof net.createConnection);

const server = net.createServer((sock: any) => {
  sock.on("data", (d: any) => {
    sock.write("echo:" + d.toString());
  });
});

server.listen(0, "127.0.0.1", () => {
  const port = server.address().port;

  // connect() as a bound value, with the connect-listener callback
  const sock = net.connect(port, "127.0.0.1", () => {
    sock.write("hello");
  });

  sock.on("data", (d: any) => {
    console.log("round trip       :", d.toString());
    sock.end();

    // createConnection() is the same factory under its other name
    const sock2 = net.createConnection(port, "127.0.0.1", () => {
      sock2.write("again");
    });
    sock2.on("data", (d2: any) => {
      console.log("createConnection :", d2.toString());
      sock2.end();
      server.close();
    });
  });

  sock.on("error", (e: any) => {
    console.log("socket error     :", e.message);
    server.close();
  });
});
