import * as dgram from "node:dgram";

function bind(socket: dgram.Socket, port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    socket.once("error", reject);
    socket.bind(port, "0.0.0.0", () => {
      socket.removeListener("error", reject);
      resolve();
    });
  });
}

function close(socket: dgram.Socket): Promise<void> {
  return new Promise((resolve) => socket.close(resolve));
}

const first = dgram.createSocket({ type: "udp4", reuseAddr: true });
const second = dgram.createSocket({ type: "udp4", reuseAddr: true });

await bind(first, 0);
const port = first.address().port;
await bind(second, port);
console.log("shared bind: true");

await close(second);
await close(first);

const replacement = dgram.createSocket("udp4");
await bind(replacement, port);
console.log("released after close: true");
await close(replacement);
