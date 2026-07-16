import { AsyncResource, createHook } from "node:async_hooks";

const targetIds = new Set<number>();
const destroyed: number[] = [];
let secondResource!: AsyncResource;
const hook = createHook({
  init(asyncId, type) {
    if (type === "DestroyQueueResource") targetIds.add(asyncId);
  },
  destroy(asyncId) {
    if (!targetIds.has(asyncId)) return;
    destroyed.push(asyncId);
    if (destroyed.length === 1) secondResource.emitDestroy();
  },
}).enable();

const firstResource = new AsyncResource("DestroyQueueResource");
secondResource = new AsyncResource("DestroyQueueResource");
const firstId = firstResource.asyncId();
const secondId = secondResource.asyncId();
firstResource.emitDestroy();

await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log(
  "destroy queue identities:",
  targetIds.size === 2,
  destroyed[0] === firstId,
  destroyed[1] === secondId,
);
console.log("destroy queue counts:", destroyed.length, new Set(destroyed).size);
hook.disable();
