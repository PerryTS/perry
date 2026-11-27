// destroy() fails the callbacks still pending. Data already handed to the
// socket fails them all with ERR_SOCKET_CLOSED_BEFORE_CONNECTION before the
// connection exists and ECANCELED once open; otherwise (only corked data, or
// only end()) each gets the stream's own ERR_STREAM_DESTROYED.
import * as net from "node:net";

const show = (label: string) => (error: any) =>
  console.log(label, error instanceof Error, Object.keys(error).join(","), error.code, error.syscall, error.message);

const peers: net.Socket[] = [];
const server = net.createServer((peer) => {
  peer.pause();
  peers.push(peer);
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const port = (server.address() as net.AddressInfo).port;

const early = net.connect(port, "127.0.0.1");
early.write("x", show("pre-connect write"));
early.end(show("pre-connect end"));
early.destroy();
await new Promise((resolve) => early.once("close", resolve));

const open = net.connect(port, "127.0.0.1");
await new Promise((resolve) => open.once("connect", resolve));
open.write(Buffer.alloc(32 * 1024 * 1024), show("in-flight write"));
open.write("y", show("queued write"));
open.end(show("end"));
setTimeout(() => open.destroy(), 20);
await new Promise((resolve) => open.once("close", resolve));

const endOnly = net.connect(port, "127.0.0.1");
endOnly.end(show("pre-connect end only"));
endOnly.destroy();
await new Promise((resolve) => endOnly.once("close", resolve));

const corkedEarly = net.connect(port, "127.0.0.1");
corkedEarly.cork();
corkedEarly.write("x", show("pre-connect corked write"));
corkedEarly.destroy();
await new Promise((resolve) => corkedEarly.once("close", resolve));

for (const make of [
  (socket: net.Socket) => {
    socket.cork();
    socket.write("x", show("corked write"));
  },
  (socket: net.Socket) => socket.end(show("open end only")),
]) {
  const socket = net.connect(port, "127.0.0.1");
  await new Promise((resolve) => socket.once("connect", resolve));
  make(socket);
  socket.destroy();
  await new Promise((resolve) => socket.once("close", resolve));
}

for (const peer of peers) peer.destroy();
server.close(() => console.log("server closed"));
