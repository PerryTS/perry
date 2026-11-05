import * as net from "node:net";

const server = net.createServer((peer) => {
  let received = "";
  peer.on("data", (chunk) => { received += chunk.toString(); });
  peer.on("end", () => {
    console.log("received", received);
    peer.end();
  });
});
server.listen(0, "127.0.0.1", () => {
  const socket = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
  socket.on("connect", () => {
    socket.cork();
    const alias: any = socket;
    alias.cork();
    console.log("corked", socket.writableCorked, alias.writableCorked);
    socket.write("one", () => console.log("callback", 1));
    alias.write("two", () => console.log("callback", 2));
    console.log("buffered", alias.writableLength);
    alias.uncork();
    console.log("nested", alias.writableCorked);
    socket.uncork();
    console.log("uncorked", alias.writableCorked);
    socket.cork();
    socket.write("three", () => console.log("callback", 3));
    // end() releases all remaining cork levels and includes the final chunk.
    socket.end("four", () => console.log("ended"));
    console.log("end corked", alias.writableCorked);
  });
  socket.on("close", () => server.close());
});
