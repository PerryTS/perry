import * as https from "node:https";

const agent = new https.Agent({ keepAlive: true, maxSockets: 5 });

const createConnection = agent.createConnection;
const keepSocketAlive = agent.keepSocketAlive;
const reuseSocket = agent.reuseSocket;
const getName = agent.getName;
const destroy = agent.destroy;
const close = agent.close;

console.log("createConnection value:", typeof createConnection);
console.log("keepSocketAlive value:", typeof keepSocketAlive);
console.log("reuseSocket value:", typeof reuseSocket);
console.log("getName value:", typeof getName);
console.log("destroy value:", typeof destroy);
console.log("close value:", typeof close);
console.log("getName detached:", getName({ host: "127.0.0.1", port: 443 }));

agent.destroy();
console.log("agent destroyed:", true);
