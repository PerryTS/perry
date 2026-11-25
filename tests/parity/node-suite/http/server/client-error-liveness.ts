import http from "node:http";
import net from "node:net";

const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 5000);
reached.unref();

const server = http.createServer((_req, res) => res.end("ok"));
server.on("clientError", (error: any, socket) => {
  console.log("clientError", error.code);
  socket.on("close", () => console.log("socket close"));
});
server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as net.AddressInfo;
  const client = net.connect({ port, host: "127.0.0.1", allowHalfOpen: true }, () => client.end("GET / HT"));
  client.on("error", () => {});
  client.on("close", () => {
    console.log("client close");
    server.close();
  });
  setTimeout(() => client.destroy(), 100);
});
process.on("exit", (code) => console.log("exit", code));
