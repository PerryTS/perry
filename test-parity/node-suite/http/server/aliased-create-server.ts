import { createServer as createServerHTTP, get } from "node:http";

function createAdaptorServer(options: any) {
  const createServer = options.createServer || createServerHTTP;
  return createServer(options.serverOptions || {}, (req: any, res: any) => {
    res.end(`alias:${req.url}`);
  });
}

const server = createAdaptorServer({});

server.listen(0, () => {
  const addr = server.address();
  get({ hostname: "127.0.0.1", port: addr.port, path: "/ok" }, (res: any) => {
    let body = "";
    res.on("data", (chunk: any) => {
      body += chunk;
    });
    res.on("end", () => {
      console.log("aliased createServer status:", String(res.statusCode));
      console.log("aliased createServer body:", body);
      server.close(() => console.log("aliased createServer closed"));
    });
  });
});
