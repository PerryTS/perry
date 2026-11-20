import { AsyncResource, createHook } from "node:async_hooks";

const resource = new AsyncResource("ParityHookScope");
const targetId = resource.asyncId();
const events: string[] = [];

const hook = createHook({
  before(asyncId) {
    if (asyncId === targetId) events.push("before");
  },
  after(asyncId) {
    if (asyncId === targetId) events.push("after");
  },
}).enable();

const returned = resource.runInAsyncScope(() => {
  events.push("callback");
  return "result";
});

hook.disable();
resource.emitDestroy();

console.log("scope events:", events.join(">"));
console.log("scope return:", returned);
