import net from "node:net";

// A 'readable' listener added after the buffer filled (and the read stopped) still drains it.
const reached = setTimeout(() => {
  console.log("still alive");
  process.exit(3);
}, 15000);
reached.unref();

const total = 2 * new net.Socket().readableHighWaterMark;
const server = net.createServer((socket) => {
  server.close();
  setTimeout(() => {
    let received = 0;
    socket.on("readable", () => {
      let chunk;
      while ((chunk = socket.read()) !== null) received += chunk.length;
    });
    socket.on("end", () => console.log("received all:", received === total));
  }, 100);
});
server.listen(0, "127.0.0.1", () => {
  const client = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1", () =>
    client.end(Buffer.alloc(total)),
  );
  client.on("close", () => console.log("client close"));
});
process.on("exit", (code) => console.log("exit", code));
