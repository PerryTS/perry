import * as http2 from "node:http2";

// A peer that ends the connection closes the client session ('close' sees
// closed and destroyed), and a request on a destroyed session fails rather
// than hanging.
const server = http2.createServer();
server.on("session", (session) => setTimeout(() => session.destroy(), 10));
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const url = `http://127.0.0.1:${(server.address() as any).port}`;
await new Promise<void>((resolve) => {
  const client = http2.connect(url);
  client.on("error", () => {});
  client.on("close", () => {
    console.log("peer close", client.closed, client.destroyed);
    resolve();
  });
});
await new Promise<void>((resolve) => {
  const client = http2.connect(url);
  client.on("connect", () => {
    client.destroy();
    const request = client.request({ ":path": "/" });
    request.on("error", () => {
      console.log("request on a destroyed session fails");
      resolve();
    });
  });
});
await new Promise<void>((resolve) => server.close(() => resolve()));
console.log("done");
