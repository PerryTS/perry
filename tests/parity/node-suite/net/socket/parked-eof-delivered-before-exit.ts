import net from "node:net";

// An EOF read in the same poll as the data before it is still delivered when nothing else holds the loop.
const server = net.createServer((socket) => {
  server.close();
  socket.on("data", (chunk) => console.log("data:", String(chunk)));
  socket.on("end", () => console.log("end"));
  socket.on("close", () => console.log("server socket close"));
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1", () =>
    client.end("a", () => client.destroy()),
  );
});
process.on("exit", (code) => console.log("exit", code));
