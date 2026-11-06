import * as https from "node:https";

const agent: any = new https.Agent();
console.log("limit:", agent.maxCachedSessions);
console.log("cache:", typeof agent._sessionCache);
if (agent._sessionCache) {
  console.log("list:", JSON.stringify(agent._sessionCache.list));
  console.log("keys:", JSON.stringify(Object.keys(agent._sessionCache.map)));
  console.log("missing:", agent._getSession("missing"));
}
agent.destroy();
