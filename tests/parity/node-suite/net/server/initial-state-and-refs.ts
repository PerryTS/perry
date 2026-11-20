import { createServer } from "node:net";

const server = createServer();

console.log(
  "initial:",
  server.listening,
  server.address(),
  server.maxConnections,
);
console.log(
  "refs:",
  server.unref() === server,
  server.ref() === server,
  typeof (server as any).hasRef,
);
console.log(
  "methods:",
  typeof server.close,
  typeof server.getConnections,
  typeof server.listen,
);

server.close();
