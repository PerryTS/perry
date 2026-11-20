import * as net from "node:net";

function cancel(socket: net.Socket, label: string, done: () => void): void {
  let callbacks = 0;
  socket.on("close", () => {
    console.log(label, "close", callbacks, socket.writableLength, socket.writableCorked);
    done();
  });
  socket.cork();
  socket.write("one", (error) => { callbacks++; console.log(label, "first", !!error); });
  socket.write("two", (error) => { callbacks++; console.log(label, "second", !!error); });
  socket.write("three", (error) => { callbacks++; console.log(label, "third", !!error); });
  socket.destroy();
  socket.destroy();
  console.log(label, "destroyed");
}

cancel(new net.Socket(), "unopened", () => {
  const server = net.createServer(() => {});
  server.listen(0, "127.0.0.1", () => {
    const socket = net.connect((server.address() as net.AddressInfo).port, "127.0.0.1");
    socket.on("connect", () => cancel(socket, "connected", () => server.close()));
  });
});
