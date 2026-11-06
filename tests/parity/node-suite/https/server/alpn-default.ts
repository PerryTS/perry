import * as https from "node:https";

const server: any = https.createServer();
const protocols = server.ALPNProtocols;
console.log("buffer:", Buffer.isBuffer(protocols));
console.log(
  "wire:",
  protocols ? JSON.stringify(Array.from(protocols)) : protocols,
);
server.close();
