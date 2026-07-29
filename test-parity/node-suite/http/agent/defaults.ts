import { Agent } from "node:http";

const agent = new Agent();
console.log("protocol/defaultPort:", agent.protocol, agent.defaultPort);
console.log("keep alive:", agent.keepAlive, agent.keepAliveMsecs);
console.log(
  "limits:",
  agent.maxSockets,
  agent.maxFreeSockets,
  agent.maxTotalSockets,
);
console.log("scheduling:", agent.scheduling);
console.log("socket count:", agent.totalSocketCount);
console.log(
  "null prototypes:",
  [agent.requests, agent.sockets, agent.freeSockets].map((value) =>
    Object.getPrototypeOf(value) === null
  ).join("|"),
);
console.log("default max sockets:", Agent.defaultMaxSockets);
agent.destroy();
