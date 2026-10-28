// A name that resolves to an address that refuses keeps the connection's own
// error: ECONNREFUSED, never ENOTFOUND. A literal address says the same.
import * as net from "node:net";

const port = Number(process.env.PEER_PORT);
const named = net.connect(port, "refuse.test");
named.on("connect", () => console.log("unexpected connect"));
named.on("error", (error: any) => {
  console.log("refuse.test", error.code, error.syscall);
  const literal = net.connect(port, "127.0.0.1");
  literal.on("connect", () => console.log("unexpected connect"));
  literal.on("error", (error: any) => console.log("127.0.0.1", error.code, error.syscall));
});
