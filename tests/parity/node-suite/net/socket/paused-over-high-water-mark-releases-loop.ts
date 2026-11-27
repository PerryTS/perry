import net from "node:net";

// A paused socket stops reading once its buffer reaches the high-water mark; owing nothing,
// it no longer holds the loop, even though the peer never ends.
const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

const server = net.createServer((socket) => {
  server.close(() => console.log("server closed"));
  socket.pause();
  socket.on("end", () => console.log("unexpected end"));
  process.on("exit", (code) => {
    console.log("buffered to the mark:", socket.readableLength >= socket.readableHighWaterMark);
    console.log("exit", code);
  });
});
const mark = new net.Socket().readableHighWaterMark;
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1", () => {
    client.write(Buffer.alloc(mark + mark / 2), () => console.log("client wrote"));
    client.unref();
  });
});

