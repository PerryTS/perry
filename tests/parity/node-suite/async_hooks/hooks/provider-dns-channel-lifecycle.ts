import { createHook, executionAsyncId } from "node:async_hooks";
import { Resolver } from "node:dns";

type Entry = { id: number; trigger: number; resource: object };
const entries: Entry[] = [];
const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "DNSCHANNEL") {
      entries.push({ id: asyncId, trigger: triggerAsyncId, resource });
    }
  },
}).enable();

const parent = executionAsyncId();
const resolver = new Resolver();
resolver.setServers(["127.0.0.1"]);
resolver.cancel();
await new Promise<void>((resolve) => setImmediate(resolve));
hook.disable();

console.log("dns channel count:", entries.length);
console.log(
  "dns channel relationship:",
  entries.length === 1 && entries[0].id > 0,
  entries.length === 1 && entries[0].trigger === parent,
  entries.length === 1 && typeof entries[0].resource === "object",
);
