const { createServer } = require("node:http");
const {
  handler,
  routeModule,
} = require("./.next/server/app/api/benchmark/route.js");

if (typeof routeModule.handle !== "function" || typeof handler !== "function") {
  throw new Error("production App Route handler exports are missing");
}

const enteredRequestIds = new Set();
const routeModuleHandle = routeModule.handle.bind(routeModule);
routeModule.handle = async (request, context) => {
  enteredRequestIds.add(request.nextUrl.searchParams.get("id") ?? "missing");
  return routeModuleHandle(request, context);
};

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
  work
    .then(() => {
      const id = new URL(request.url, `http://${hostname}:${port}`).searchParams.get("id") ?? "missing";
      if (!enteredRequestIds.delete(id)) {
        throw new Error(`${id}: generated handler bypassed routeModule.handle`);
      }
    })
    .catch((error) => {
      console.error(error);
      if (!response.headersSent) response.statusCode = 500;
      response.end();
    });
});

server.listen(port, hostname, () => {
  console.log(`PERRY_NEXT_APP_ROUTE_READY http://${hostname}:${port}`);
});
