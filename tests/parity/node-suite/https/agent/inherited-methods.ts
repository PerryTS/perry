import * as https from "node:https";

const agent = new https.Agent();
console.log("createConnection:", typeof agent.createConnection);
console.log("keepSocketAlive:", typeof agent.keepSocketAlive);
console.log("reuseSocket:", typeof agent.reuseSocket);
console.log("getName:", typeof agent.getName);
console.log("destroy:", typeof agent.destroy);
console.log("close:", typeof (agent as any).close);
agent.destroy();
