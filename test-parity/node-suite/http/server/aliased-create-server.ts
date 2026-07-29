import { createServer as createServerHTTP, get } from "node:http";

function createAdaptorServer(options: any) {
  const createServer = options.createServer || createServerHTTP;
  return createServer(options.serverOptions || {}, (req: any, res: any) => {
    res.end(`alias:${req.url}`);
  });
}

const server = createAdaptorServer({});

await new Promise<void>((resolve, reject) => {
  server.once("error", reject);
  server.listen(0, "127.0.0.1", resolve);
});
const addr = server.address();
if (!addr || typeof addr === "string") throw new Error("missing address");

try {
  await new Promise<void>((resolve, reject) => {
    const req = get(
      { hostname: "127.0.0.1", port: addr.port, path: "/ok" },
      (res: any) => {
        let body = "";
        res.on("data", (chunk: any) => {
          body += chunk;
        });
        res.once("error", reject);
        res.on("end", () => {
          console.log("aliased createServer status:", String(res.statusCode));
          console.log("aliased createServer body:", body);
          resolve();
        });
      },
    );
    req.once("error", reject);
  });
} finally {
  await new Promise<void>((resolve) => {
    server.close(() => {
      console.log("aliased createServer closed");
      resolve();
    });
  });
}
