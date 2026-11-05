import { AsyncResource, createHook } from "node:async_hooks";

const resource = new AsyncResource("ParityScopeThrow");
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

try {
  resource.runInAsyncScope(() => {
    events.push("callback");
    throw new Error("expected");
  });
} catch (error) {
  console.log("scope throw error:", (error as Error).message);
}

console.log("scope throw events:", events.join(">"));

hook.disable();
resource.emitDestroy();
