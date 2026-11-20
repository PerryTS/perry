import { Agent } from "node:http";

// Mirrors Node's test-http-agent-keep-alive-timeout-buffer.js constructor
// assertions: valid values are retained and invalid/missing values use 1000.
const configured = new Agent({ agentKeepAliveTimeoutBuffer: 1500 });
console.log(configured.agentKeepAliveTimeoutBuffer);

const negative = new Agent({ agentKeepAliveTimeoutBuffer: -100 });
console.log(negative.agentKeepAliveTimeoutBuffer);

const infinite = new Agent({ agentKeepAliveTimeoutBuffer: Infinity });
console.log(infinite.agentKeepAliveTimeoutBuffer);

const defaulted = new Agent();
console.log(defaulted.agentKeepAliveTimeoutBuffer);

configured.destroy();
negative.destroy();
infinite.destroy();
defaulted.destroy();
