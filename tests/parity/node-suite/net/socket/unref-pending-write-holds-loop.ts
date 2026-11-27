import net from "node:net";

// An unreferenced socket still keeps the loop alive while a write is owed, as libuv's write request does.
const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

const server = net.createServer((socket) => {
  server.close();
  socket.unref();
  socket.resume();
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1", () => {
    client.unref();
    client.write(Buffer.alloc(8 * 1024 * 1024), () => console.log("client flushed"));
  });
});
process.on("exit", (code) => console.log("exit", code));
