// A socket still receiving data, destroyed in the check phase, closes in that
// iteration's close-callbacks phase, before a timer scheduled beside the
// destroy: data read for it in the meantime is discarded with the handle.
import * as net from "node:net";

const server = net.createServer((peer) => {
  const flood = setInterval(() => peer.write("x".repeat(512)), 1);
  peer.on("error", () => {});
  peer.on("close", () => clearInterval(flood));
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address() as net.AddressInfo;

function round(label: string): Promise<void> {
  return new Promise((resolve) => {
    const socket = net.connect(port, "127.0.0.1");
    socket.once("data", () => {
      setImmediate(() => {
        socket.on("close", () => console.log(label, "close"));
        socket.destroy();
        setTimeout(() => {
          console.log(label, "timeout");
          resolve();
        }, 0);
        const until = performance.now() + 5;
        while (performance.now() < until) {}
      });
    });
  });
}

await round("await");
round("callback").then(() => server.close());
