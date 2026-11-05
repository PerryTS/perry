// Descriptor numbers are reused: after the first listener is closed, the
// connection accepted on the second takes its number, and must not report the
// closed listener's announced address as its own.
import * as net from "node:net";

let firstPort = -1;
const second = net.createServer((socket) => {
  console.log("local port", socket.localPort === firstPort ? "is the closed listener's" : "is not the closed listener's");
  socket.end();
  second.close();
});
const first = net.createServer(() => {});
first.listen(0, () => {
  firstPort = (first.address() as any).port;
  second.listen(0, () => {
    first.close(() => console.log("ready"));
  });
});
