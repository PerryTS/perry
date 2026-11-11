import * as https from "node:https";

const ALPNCallback = () => "http/1.1";
const server: any = https.createServer({ ALPNCallback });
console.log("callback:", server.ALPNCallback === ALPNCallback);
console.log("protocols:", server.ALPNProtocols);
server.close();
