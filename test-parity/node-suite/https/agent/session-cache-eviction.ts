import * as https from "node:https";

const agent: any = new https.Agent({ maxCachedSessions: 2 });
console.log("method:", typeof agent._cacheSession);
if (agent._cacheSession) {
  agent._cacheSession("a", Buffer.from("one"));
  agent._cacheSession("b", Buffer.from("two"));
  agent._cacheSession("c", Buffer.from("three"));
  console.log("list:", JSON.stringify(agent._sessionCache.list));
  console.log("keys:", JSON.stringify(Object.keys(agent._sessionCache.map)));
  console.log("a:", agent._getSession("a"));
  console.log("c:", agent._getSession("c").toString());
}
agent.destroy();
