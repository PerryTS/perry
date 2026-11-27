import * as http2 from "node:http2";

// A client session's graceful close() emits 'close' once its socket has
// closed, not from the call: a check-phase callback runs first. Its callback
// is a 'close' listener, in registration order.
const server = http2.createServer();
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
client.on("error", () => {});
await new Promise((resolve) => client.on("connect", resolve));

const closed = new Promise<void>((resolve) => {
  client.on("close", () => console.log("session close", client.closed, client.destroyed));
  client.close(() => {
    console.log("close callback");
    resolve();
  });
});
client.on("close", () => console.log("listener added after close()"));
console.log("after close()", client.closed, client.destroyed);
setImmediate(() => console.log("immediate"));
await closed;
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
