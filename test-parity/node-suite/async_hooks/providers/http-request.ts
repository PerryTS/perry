import { createServer, request } from "node:http";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const server = createServer((incoming, response) => {
  response.end("http-payload");
});

await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
if (!address || typeof address === "string")
  throw new Error("missing server address");

const body = await storage.run(
  "http-request",
  () =>
    new Promise<string>((resolve, reject) => {
      const req = request(
        { host: "127.0.0.1", port: address.port, path: "/" },
        (response) => {
          console.log("http response store:", storage.getStore());
          const chunks: string[] = [];
          response.on("data", (chunk) => {
            console.log("http data store:", storage.getStore());
            chunks.push(String(chunk));
          });
          response.on("end", () => {
            console.log("http end store:", storage.getStore());
            resolve(chunks.join(""));
          });
        },
      );
      req.on("error", reject);
      req.end();
    }),
);

await new Promise<void>((resolve, reject) =>
  server.close((error) => (error ? reject(error) : resolve())),
);
console.log("http body:", body);
console.log("http outside:", String(storage.getStore()));
