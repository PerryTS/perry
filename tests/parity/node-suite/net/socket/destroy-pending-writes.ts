// destroy() fails the writes still pending: before the connection exists they
// get ERR_SOCKET_CLOSED_BEFORE_CONNECTION, on an open handle ECANCELED.
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

for (const peer of peers) peer.destroy();
server.close(() => console.log("server closed"));
