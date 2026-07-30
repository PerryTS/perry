import * as https from "node:https";
import * as tls from "node:tls";

const server = https.Server();
console.log(
  "instances:",
  server instanceof https.Server,
  server instanceof tls.Server,
);
console.log(
  "constructor inheritance:",
  Object.getPrototypeOf(https.Server) === tls.Server,
);
console.log(
  "prototype inheritance:",
  Object.getPrototypeOf(https.Server.prototype) === tls.Server.prototype,
);
console.log("without new:", server instanceof https.Server);
server.close();
