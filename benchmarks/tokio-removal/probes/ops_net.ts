// N ping/pong round trips over one node:net connection.
import * as net from "node:net";
const N = Number(process.argv[2] || 100);
const server = net.createServer((s) => s.on("data", (d) => s.write(d)));
server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as net.AddressInfo).port;
  let n = 0;
  const c = net.connect(port, "127.0.0.1", () => c.write("p"));
  c.on("data", () => {
    if (++n < N) c.write("p");
    else { c.end(); server.close(); console.log("net", n); }
  });
});
