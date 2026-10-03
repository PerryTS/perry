import { createServer } from "node:http2";
import { connect } from "node:net";

let accepted = 0;
let socketClosed = 0;
let requests = 0;
let clientClosed = false;
let serverClosed = false;
const timeout = setTimeout(() => {
  console.log("timeout");
  process.exit(1);
}, 2000);

const server = createServer((_req, res) => {
  requests++;
  res.end();
});
server.on("connection", (socket) => {
  accepted++;
  socket.on("close", () => socketClosed++);
  socket.destroy();
  socket.destroy();
});

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = connect((server.address() as any).port, "127.0.0.1");
client.on("error", () => {});
await new Promise<void>((resolve) => client.on("close", resolve));
clientClosed = true;
await new Promise<void>((resolve) => server.close(resolve));
serverClosed = true;
clearTimeout(timeout);
console.log("accepted", accepted);
console.log("socket close", socketClosed);
console.log("client close", clientClosed);
console.log("server close", serverClosed);
console.log("requests", requests);
