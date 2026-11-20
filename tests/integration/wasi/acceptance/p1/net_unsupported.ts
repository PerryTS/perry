// The baseline net contract of the wasip1 output (docs/WASI_P1_OUTPUT.md):
// without a host that provides networking, name lookup and outbound
// connections are "not supported" and there is no pre-opened listener.
import * as dns from "node:dns";
import * as net from "node:net";

dns.lookup("example.com", (error: any) => {
  console.log("dns.lookup", error ? `${error.code} ${error.message}` : "no error");
  const socket = net.connect(9, "127.0.0.1");
  socket.on("connect", () => {
    console.log("net.connect", "unexpected connect");
    socket.destroy();
  });
  socket.on("error", (error: any) => {
    console.log("net.connect", error.code);
    const server = net.createServer(() => {});
    server.on("error", (error: any) => console.log("listen", error.code));
    server.listen(0, () => console.log("listen", "unexpected listen"));
  });
});
