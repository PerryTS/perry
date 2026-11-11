import { Agent } from "node:http";

const agent = new Agent({
  keepAlive: true,
  keepAliveMsecs: 250,
  maxSockets: 3,
  maxFreeSockets: 2,
  maxTotalSockets: 4,
  scheduling: "fifo",
  agentKeepAliveTimeoutBuffer: 125,
});

console.log(
  "values:",
  [
    agent.keepAlive,
    agent.keepAliveMsecs,
    agent.maxSockets,
    agent.maxFreeSockets,
    agent.maxTotalSockets,
    agent.scheduling,
    agent.agentKeepAliveTimeoutBuffer,
  ].join("|"),
);
console.log(
  "options copied:",
  agent.options.keepAlive,
  agent.options.noDelay,
  agent.options.path,
);
console.log(
  "options prototype:",
  Object.getPrototypeOf(agent.options) === null,
);
agent.destroy();
