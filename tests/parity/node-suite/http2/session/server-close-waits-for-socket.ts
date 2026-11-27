import * as http2 from "node:http2";

// A server session's graceful close() emits 'close' once its socket has
// closed, not from the call: a check-phase callback runs first.
const server = http2.createServer();
const opened = new Promise<http2.ServerHttp2Session>((resolve) => {
  server.on("session", resolve);
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
client.on("error", () => {});
const session = await opened;

const closed = new Promise<void>((resolve) => {
  session.on("close", () => console.log("session close", session.closed, session.destroyed));
  session.close(() => {
    console.log("close callback");
    resolve();
  });
});
console.log("after close()", session.closed, session.destroyed);
setImmediate(() => console.log("immediate"));
await closed;
client.destroy();
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
