import * as https from "node:https";

const agent: any = new https.Agent({ maxCachedSessions: 0 });
console.log("limit:", agent.maxCachedSessions);
console.log("method:", typeof agent._cacheSession);
if (agent._cacheSession) {
  agent._cacheSession("a", Buffer.from("one"));
  console.log("list:", JSON.stringify(agent._sessionCache.list));
  console.log("keys:", JSON.stringify(Object.keys(agent._sessionCache.map)));
  console.log("value:", agent._getSession("a"));
}
agent.destroy();
