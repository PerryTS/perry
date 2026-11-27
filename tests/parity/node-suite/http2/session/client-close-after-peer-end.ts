import * as http2 from "node:http2";

// close() from the 'goaway' of a server that closed its session emits
// 'close' once the socket has closed, after a check-phase callback.
const server = http2.createServer();
server.on("session", (session) => setTimeout(() => session.close(), 10));
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
client.on("error", () => {});
await new Promise<void>((resolve) => {
  let seen = false;
  client.on("goaway", () => {
    if (seen) return;
    seen = true;
    console.log("goaway");
    client.close(() => console.log("close cb"));
    setImmediate(() => console.log("immediate"));
  });
  client.on("close", () => {
    console.log("client close");
    resolve();
  });
});
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
