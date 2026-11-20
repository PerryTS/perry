import * as https from "node:https";

const agent = new https.Agent();
console.log("defaultPort:", agent.defaultPort);
console.log("protocol:", agent.protocol);
console.log("keepAlive:", agent.keepAlive);
console.log("keepAliveMsecs:", agent.keepAliveMsecs);
console.log("maxCachedSessions:", agent.maxCachedSessions);
console.log("maxFreeSockets:", agent.maxFreeSockets);
console.log("maxSockets:", agent.maxSockets);
agent.destroy();
