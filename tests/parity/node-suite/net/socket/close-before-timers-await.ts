// Under a top-level await, a socket destroyed in the check phase still closes
// in that iteration's close-callbacks phase, before the next iteration's
// timers, whether it had a handle or was still resolving.
import * as net from "node:net";

const server = net.createServer(() => {});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const { port } = server.address() as net.AddressInfo;

for (const [label, host] of [["connected", "127.0.0.1"], ["resolving", "localhost"]]) {
  const socket = net.connect(host === "127.0.0.1" ? port : 1, host);
  socket.on("error", () => {});
  if (host === "127.0.0.1") await new Promise((resolve) => socket.once("connect", resolve));
  await new Promise<void>((resolve) => {
    setImmediate(() => {
      socket.on("close", () => console.log(label, "close"));
      socket.destroy();
      setTimeout(() => {
        console.log(label, "timeout");
        resolve();
      }, 0);
      const until = performance.now() + 3;
      while (performance.now() < until) {}
    });
    setImmediate(() => console.log(label, "sibling immediate"));
  });
}
server.close();
