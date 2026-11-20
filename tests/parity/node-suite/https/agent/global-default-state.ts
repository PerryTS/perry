import * as https from "node:https";

console.log("instance:", https.globalAgent instanceof https.Agent);
console.log("defaultPort:", https.globalAgent.defaultPort);
console.log("protocol:", https.globalAgent.protocol);
console.log("keepAlive:", https.globalAgent.keepAlive);
console.log("maxCachedSessions:", https.globalAgent.maxCachedSessions);
