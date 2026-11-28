import * as http2 from "node:http2";
import * as net from "node:net";

// close() before the socket connects, with no request and a peer that never
// speaks HTTP/2, still ends the session once the socket connects, well
// before the 10 s SETTINGS timeout would.
const sockets: net.Socket[] = [];
const peer = net.createServer((socket) => {
  sockets.push(socket);
  socket.on("error", () => {});
});
await new Promise<void>((resolve) => peer.listen(0, "127.0.0.1", resolve));
const started = Date.now();
await new Promise<void>((resolve) => {
  const client = http2.connect(`http://127.0.0.1:${(peer.address() as any).port}`);
  client.on("error", () => {});
  client.close(() => {
    console.log("close cb", client.closed, client.destroyed, Date.now() - started < 5000);
    resolve();
  });
});
for (const socket of sockets) socket.destroy();
await new Promise<void>((resolve) => peer.close(() => resolve()));
console.log("done");
