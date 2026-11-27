import * as http2 from "node:http2";
import * as net from "node:net";

// A destroy() from the 'error' listener of a refused connect does not emit
// 'close' ahead of the socket's close.
const probe = net.createServer();
await new Promise<void>((resolve) => probe.listen(0, "127.0.0.1", resolve));
const port = (probe.address() as any).port;
await new Promise<void>((resolve) => probe.close(() => resolve()));

await new Promise<void>((resolve) => {
  const client = http2.connect(`http://127.0.0.1:${port}`);
  client.on("error", (error: any) => {
    console.log("error", error.code);
    client.destroy();
    setImmediate(() => console.log("immediate"));
  });
  client.on("close", () => {
    console.log("close", client.destroyed);
    resolve();
  });
});
console.log("done");
