import { AsyncResource, createHook } from "node:async_hooks";

const firstEvents: string[] = [];
const secondEvents: string[] = [];
let targetId = -1;

const first = createHook({
  init(asyncId, type) {
    if (type === "ParityMultipleHooks") {
      targetId = asyncId;
      firstEvents.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === targetId) firstEvents.push("before");
  },
  after(asyncId) {
    if (asyncId === targetId) firstEvents.push("after");
  },
}).enable();
const second = createHook({
  init(asyncId, type) {
    if (type === "ParityMultipleHooks") {
      targetId = asyncId;
      secondEvents.push("init");
    }
  },
  before(asyncId) {
    if (asyncId === targetId) secondEvents.push("before");
  },
  after(asyncId) {
    if (asyncId === targetId) secondEvents.push("after");
  },
}).enable();
const resource = new AsyncResource("ParityMultipleHooks");

resource.runInAsyncScope(() => {
  console.log("both hooks callback");
});
console.log("first hook events:", firstEvents.join(">"));
console.log("second hook events:", secondEvents.join(">"));

first.disable();
resource.runInAsyncScope(() => {
  console.log("second hook callback");
});
console.log("first hook after disable:", firstEvents.join(">"));
console.log("second hook after first disable:", secondEvents.join(">"));

second.disable();
resource.emitDestroy();
