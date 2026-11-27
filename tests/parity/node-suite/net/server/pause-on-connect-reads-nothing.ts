import net from "node:net";

// pauseOnConnect hands the connection over unread: the socket reads nothing until resumed.
const server = net.createServer({ pauseOnConnect: true }, (socket) => {
  server.close();
  setTimeout(() => {
    console.log("before resume:", socket.readableLength, socket.bytesRead);
    socket.on("data", (chunk) => console.log("data:", String(chunk)));
    socket.on("end", () => socket.end());
    socket.resume();
  }, 100);
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1", () =>
    client.end("hello"),
  );
  client.resume();
  client.on("close", () => console.log("client close"));
});
