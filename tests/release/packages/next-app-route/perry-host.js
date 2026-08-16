const { createServer } = require("node:http");
const {
  handler,
  routeModule,
} = require("./.next/server/app/api/benchmark/route.js");

if (typeof routeModule.handle !== "function" || typeof handler !== "function") {
  throw new Error("production App Route handler exports are missing");
}

const pending = new Set();
const port = Number(process.env.PORT ?? "3100");
const hostname = process.env.HOSTNAME ?? "127.0.0.1";

const server = createServer((request, response) => {
  const work = handler(request, response, {
    waitUntil(promise) {
      pending.add(promise);
      promise.finally(() => pending.delete(promise));
    },
  });
  work.catch((error) => {
    console.error(error);
    if (!response.headersSent) response.statusCode = 500;
    response.end();
  });
});

server.listen(port, hostname, () => {
  console.log(`PERRY_NEXT_APP_ROUTE_READY http://${hostname}:${port}`);
});
