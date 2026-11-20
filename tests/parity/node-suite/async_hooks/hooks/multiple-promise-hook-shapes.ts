import { createHook } from "node:async_hooks";

type Lifecycle = {
  init: number;
  before: number;
  after: number;
  resolve: number;
};
const ids = new WeakMap<object, number>();
const lifecycle = new Map<number, Lifecycle>();
const firstHook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    if (type !== "PROMISE") return;
    ids.set(resource, asyncId);
    lifecycle.set(asyncId, { init: 1, before: 0, after: 0, resolve: 0 });
  },
  before(asyncId) {
    const state = lifecycle.get(asyncId);
    if (state) state.before++;
  },
  after(asyncId) {
    const state = lifecycle.get(asyncId);
    if (state) state.after++;
  },
  promiseResolve(asyncId) {
    const state = lifecycle.get(asyncId);
    if (state) state.resolve++;
  },
}).enable();
const secondHook = createHook({
  init() {},
  destroy() {},
}).enable();

const parent = Promise.resolve(41);
const child = parent.then((value) => value + 1);
const value = await child;
await new Promise<void>((resolve) => setImmediate(resolve));

const parentId = ids.get(parent) ?? -1;
const childId = ids.get(child) ?? -1;
const parentState = lifecycle.get(parentId);
const childState = lifecycle.get(childId);
console.log(
  "multiple promise hook identities:",
  parentId > 0,
  childId > 0,
  parentId !== childId,
);
console.log(
  "multiple promise parent lifecycle:",
  parentState?.init === 1,
  parentState?.resolve === 1,
);
console.log(
  "multiple promise child lifecycle:",
  childState?.init === 1,
  childState?.before === 1,
  childState?.after === 1,
  childState?.resolve === 1,
  value,
);
firstHook.disable();
secondHook.disable();
