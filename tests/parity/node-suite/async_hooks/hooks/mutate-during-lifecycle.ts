import { AsyncResource, createHook } from "node:async_hooks";

let targetId = -1;
let phase = 0;
const firstEvents: string[] = [];
const secondEvents: string[] = [];
const thirdEvents: string[] = [];

const first = createHook({
  init(asyncId, type) {
    if (type === "ParityHookMutation") {
      targetId = asyncId;
      firstEvents.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === targetId) firstEvents.push(`before-${phase}`);
  },
  after(asyncId) {
    if (asyncId === targetId) firstEvents.push(`after-${phase}`);
  },
  destroy(asyncId) {
    if (asyncId === targetId) firstEvents.push("destroy");
  },
});
const second = createHook({
  init(asyncId, type) {
    if (type === "ParityHookMutation") secondEvents.push("init");
  },
  before(asyncId) {
    if (asyncId !== targetId) return;
    secondEvents.push(`before-${phase}`);
    if (phase === 2) first.disable();
  },
  after(asyncId) {
    if (asyncId === targetId) secondEvents.push(`after-${phase}`);
  },
  destroy(asyncId) {
    if (asyncId === targetId) secondEvents.push("destroy");
  },
});
const third = createHook({
  init(asyncId, type) {
    if (type === "ParityHookMutation") thirdEvents.push("init");
  },
  before(asyncId) {
    if (asyncId !== targetId) return;
    thirdEvents.push(`before-${phase}`);
    if (phase === 1) second.enable();
  },
  after(asyncId) {
    if (asyncId !== targetId) return;
    thirdEvents.push(`after-${phase}`);
    if (phase === 2) third.disable();
  },
  destroy(asyncId) {
    if (asyncId === targetId) thirdEvents.push("destroy");
  },
});

first.enable();
third.enable();
const resource = new AsyncResource("ParityHookMutation");
phase = 1;
resource.runInAsyncScope(() => {});
phase = 2;
resource.runInAsyncScope(() => {});
resource.emitDestroy();
await new Promise<void>((resolve) => setImmediate(resolve));
await new Promise<void>((resolve) => setImmediate(resolve));
second.disable();

console.log("mutated first hook:", firstEvents.join(">"));
console.log("mutated second hook:", secondEvents.join(">"));
console.log("mutated third hook:", thirdEvents.join(">"));
