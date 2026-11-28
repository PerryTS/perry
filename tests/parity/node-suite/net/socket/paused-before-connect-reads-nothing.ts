import net from "node:net";

// A client paused before it connects does not start reading until it is resumed.
const server = net.createServer((socket) => {
  server.close();
  socket.end("world");
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
  client.pause();
  client.on("connect", () =>
    setTimeout(() => {
      console.log("before resume:", client.readableLength, client.bytesRead);
      client.on("data", (chunk) => console.log("data:", String(chunk)));
      client.on("end", () => console.log("end"));
      client.resume();
    }, 100),
  );
  client.on("close", () => console.log("client close"));
});
