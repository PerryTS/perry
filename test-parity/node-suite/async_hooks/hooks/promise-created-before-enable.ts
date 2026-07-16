import { createHook, executionAsyncId } from "node:async_hooks";

type PromiseInfo = { asyncId: number; triggerAsyncId: number };
const parent = Promise.resolve("parent");
const resources = new Map<object, PromiseInfo>();
const observedIds = new Set<number>();
const beforeIds = new Set<number>();
const afterIds = new Set<number>();
let executionMatches = false;

const hook = createHook({
  init(asyncId, type, triggerAsyncId, resource) {
    if (type === "PROMISE") {
      resources.set(resource, { asyncId, triggerAsyncId });
      observedIds.add(asyncId);
    }
  },
  before(asyncId) {
    beforeIds.add(asyncId);
  },
  after(asyncId) {
    afterIds.add(asyncId);
  },
}).enable();

const child = parent.then((value) => {
  executionMatches = executionAsyncId() === resources.get(child)?.asyncId;
  return `${value}:child`;
});
const value = await child;
const childInfo = resources.get(child);
hook.disable();

console.log("late hook child supplied:", !!childInfo);
console.log(
  "late hook parent init absent:",
  !observedIds.has(childInfo?.triggerAsyncId ?? -1),
);
console.log(
  "late hook trigger positive:",
  (childInfo?.triggerAsyncId ?? -1) > 0,
);
console.log("late hook execution relation:", executionMatches);
console.log(
  "late hook child lifecycle:",
  beforeIds.has(childInfo?.asyncId ?? -1),
  afterIds.has(childInfo?.asyncId ?? -1),
);
console.log("late hook child value:", value);
