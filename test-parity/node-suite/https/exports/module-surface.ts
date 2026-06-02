// #3697: node:https top-level export surface (request/get/Agent/Server/createServer).
import * as https from "node:https";
import { Agent, Server, createServer, get, request } from "node:https";

console.log("request:", typeof https.request, https.request.length);
console.log("get:", typeof https.get, https.get.length);
console.log("Agent:", typeof https.Agent, https.Agent.length);
console.log("Server:", typeof https.Server);
console.log("createServer:", typeof https.createServer);
console.log("globalAgent:", typeof https.globalAgent);
console.log("named request:", typeof request, request === https.request);
console.log("named get:", typeof get, get === https.get);
console.log("named Agent:", typeof Agent, Agent === https.Agent);
console.log("named Server:", typeof Server);
console.log("named createServer:", typeof createServer);
