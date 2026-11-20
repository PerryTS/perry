import * as https from "node:https";

const agent = new https.Agent({
  defaultPort: 8443,
  keepAlive: true,
  keepAliveMsecs: 250,
  maxCachedSessions: 7,
  maxFreeSockets: 3,
  maxSockets: 5,
  protocol: "custom:",
});
console.log("defaultPort:", agent.defaultPort);
console.log("protocol:", agent.protocol);
console.log("keepAlive:", agent.keepAlive);
console.log("keepAliveMsecs:", agent.keepAliveMsecs);
console.log("maxCachedSessions:", agent.maxCachedSessions);
console.log("maxFreeSockets:", agent.maxFreeSockets);
console.log("maxSockets:", agent.maxSockets);
agent.destroy();
