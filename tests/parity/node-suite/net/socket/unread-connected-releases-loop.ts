import net from "node:net";

// A connected socket that never started reading and owes no write does not hold the loop.
const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

const server = net.createServer((socket) => {
  server.close();
  socket.unref();
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
  client.pause();
  client.on("connect", () => console.log("connected"));
});
process.on("exit", (code) => console.log("exit", code));
