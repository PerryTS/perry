import { createServer, request } from "node:http";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const server = createServer((incoming, response) => {
  incoming.resume();
  response.end("ok");
});

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("missing server address");

await storage.run(
  "http-client-events",
  () =>
    new Promise<void>((resolve, reject) => {
      const req = request({
        host: "127.0.0.1",
        port: address.port,
        method: "POST",
      });
      req.on("response", (response) => {
        console.log("client response event store:", storage.getStore());
        response.resume();
      });
      req.on("finish", () => {
        console.log("client finish event store:", storage.getStore());
      });
      req.on("close", () => {
        console.log("client close event store:", storage.getStore());
        resolve();
      });
      req.on("error", reject);
      req.end("request-body");
    }),
);

await new Promise<void>((resolve, reject) =>
  server.close((error) => (error ? reject(error) : resolve())),
);
console.log("http client outside:", String(storage.getStore()));
