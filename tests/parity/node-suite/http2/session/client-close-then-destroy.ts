import * as http2 from "node:http2";

// close() then destroy() on a drained client session: 'close' still waits
// for the socket's close, after the check phase.
const server = http2.createServer();
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
await new Promise<void>((resolve) => {
  client.on("close", () => {
    console.log("client close");
    server.close(() => resolve());
  });
  client.on("connect", () => {
    client.close(() => console.log("close cb"));
    client.destroy();
    setImmediate(() => console.log("immediate"));
  });
});
console.log("done");
