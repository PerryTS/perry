import * as https from "node:https";

const agent: any = new https.Agent({ maxCachedSessions: 2 });
console.log("method:", typeof agent._cacheSession);
if (agent._cacheSession) {
  agent._cacheSession("a", Buffer.from("one"));
  agent._cacheSession("b", Buffer.from("two"));
  agent._cacheSession("a", Buffer.from("updated"));
  console.log("list:", JSON.stringify(agent._sessionCache.list));
  console.log("a:", agent._getSession("a").toString());
  agent._evictSession("missing");
  agent._evictSession("b");
  console.log(
    "after evict:",
    JSON.stringify(agent._sessionCache.list),
    JSON.stringify(Object.keys(agent._sessionCache.map)),
  );
}
agent.destroy();
