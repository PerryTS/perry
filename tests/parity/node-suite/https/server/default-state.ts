import * as https from "node:https";

const server = https.createServer();
console.log("httpAllowHalfOpen:", (server as any).httpAllowHalfOpen);
console.log("maxHeadersCount:", server.maxHeadersCount);
console.log("timeout:", server.timeout);
server.close();
