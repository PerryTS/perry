import * as http from "node:http";
import * as https from "node:https";

const agent = new https.Agent();
console.log(
  "instances:",
  agent instanceof https.Agent,
  agent instanceof http.Agent,
);
console.log(
  "constructor inheritance:",
  Object.getPrototypeOf(https.Agent) === http.Agent,
);
console.log(
  "prototype inheritance:",
  Object.getPrototypeOf(https.Agent.prototype) === http.Agent.prototype,
);
console.log("without new:", (https.Agent as any)() instanceof https.Agent);
agent.destroy();
