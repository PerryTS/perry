import { Agent } from "node:http";

const agent = new Agent({ keepAlive: true });
console.log(
  "pool keys:",
  Object.keys(agent.sockets).length,
  Object.keys(agent.freeSockets).length,
  Object.keys(agent.requests).length,
);
console.log("destroy return:", String(agent.destroy()));
console.log(
  "pool keys after:",
  Object.keys(agent.sockets).length,
  Object.keys(agent.freeSockets).length,
  Object.keys(agent.requests).length,
);
console.log("close identity:", agent.close === agent.destroy);
