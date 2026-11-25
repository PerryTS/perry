import http from "node:http";
import net from "node:net";

const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

let client: net.Socket;
const server = http.createServer((_req, res) => res.end("ok"));
server.on("clientError", (error: any, socket) => {
  console.log("clientError", error.code);
  socket.on("close", () => console.log("socket close"));
  client.destroy();
});
server.listen(0, "127.0.0.1", () => {
  const { port } = server.address() as net.AddressInfo;
  client = net.connect({ port, host: "127.0.0.1", allowHalfOpen: true }, () => client.end("GET / HT"));
  client.on("error", () => {});
  client.on("close", () => {
    console.log("client close");
    server.close();
  });
});
process.on("exit", (code) => console.log("exit", code));
