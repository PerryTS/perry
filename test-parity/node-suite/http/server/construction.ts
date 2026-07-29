import { createServer, Server } from "node:http";
import { Server as NetServer } from "node:net";

const called = () => {};
const direct = Server(called);
const created = createServer(called);
const constructed = new Server(called);

for (const server of [direct, created, constructed]) {
  console.log(
    "instance:",
    server instanceof Server,
    server instanceof NetServer,
  );
  console.log("listener:", server.listeners("request")[0] === called);
  console.log(
    "defaults:",
    server.listening,
    server.timeout,
    server.maxHeadersCount,
    server.maxRequestsPerSocket,
  );
}
