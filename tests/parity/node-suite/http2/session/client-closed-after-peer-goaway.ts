import * as http2 from "node:http2";

// A server's graceful close (GOAWAY NO_ERROR) closes the client session too:
// its 'goaway' listener still sees it open, its 'close' sees it closed.
const server = http2.createServer();
server.on("session", (session) => setTimeout(() => session.close(), 10));
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const client = http2.connect(`http://127.0.0.1:${(server.address() as any).port}`);
client.on("error", () => {});
let seen = false;
client.on("goaway", (code: number) => {
  if (seen) return;
  seen = true;
  console.log("goaway", code, client.closed);
});
await new Promise<void>((resolve) => {
  client.on("close", () => {
    console.log("close", client.closed, client.destroyed);
    resolve();
  });
});
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
