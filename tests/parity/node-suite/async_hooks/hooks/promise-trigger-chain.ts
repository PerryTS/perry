import { createHook, executionAsyncId, triggerAsyncId } from "node:async_hooks";

type PromiseInfo = { asyncId: number; triggerAsyncId: number };
const resources = new Map<object, PromiseInfo>();
const beforeIds = new Set<number>();
const afterIds = new Set<number>();
let callbackExecutionMatches = false;
let callbackTriggerMatches = false;

const hook = createHook({
  init(asyncId, type, triggerId, resource) {
    if (type === "PROMISE") {
      resources.set(resource, { asyncId, triggerAsyncId: triggerId });
    }
  },
  before(asyncId) {
    beforeIds.add(asyncId);
  },
  after(asyncId) {
    afterIds.add(asyncId);
  },
}).enable();

const parent = Promise.resolve(42);
const child = parent.then((value) => {
  const parentInfo = resources.get(parent);
  const childInfo = resources.get(child);
  callbackExecutionMatches = executionAsyncId() === childInfo?.asyncId;
  callbackTriggerMatches =
    triggerAsyncId() === parentInfo?.asyncId &&
    childInfo?.triggerAsyncId === parentInfo?.asyncId;
  return value + 1;
});
const value = await child;
const parentInfo = resources.get(parent);
const childInfo = resources.get(child);
hook.disable();

console.log("promise chain resources supplied:", !!parentInfo, !!childInfo);
console.log("promise chain trigger relation:", callbackTriggerMatches);
console.log("promise chain execution relation:", callbackExecutionMatches);
console.log(
  "promise chain lifecycle:",
  beforeIds.has(childInfo?.asyncId ?? -1),
  afterIds.has(childInfo?.asyncId ?? -1),
);
console.log("promise chain value:", value);
