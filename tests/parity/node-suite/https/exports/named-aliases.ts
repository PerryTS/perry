import * as https from "node:https";
import {
  Agent,
  createServer,
  get,
  globalAgent,
  request,
  Server,
} from "node:https";

console.log("Agent:", Agent === https.Agent);
console.log("Server:", Server === https.Server);
console.log("createServer:", createServer === https.createServer);
console.log("get:", get === https.get);
console.log("globalAgent:", globalAgent === https.globalAgent);
console.log("request:", request === https.request);
