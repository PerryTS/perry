import net from "node:net";

// A socket paused before it flows still reads until its buffer is full, so it sees the peer's EOF.
const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

const server = net.createServer((socket) => {
  socket.pause();
  socket.on("end", () => console.log("server end"));
  socket.on("close", () => console.log("server socket close"));
  server.close();
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
client.on("connect", () => setTimeout(() => client.end(), 20));
client.on("close", () => console.log("client close"));
process.on("exit", (code) => console.log("exit", code));
