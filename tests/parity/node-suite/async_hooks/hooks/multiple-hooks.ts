import { AsyncResource, createHook } from "node:async_hooks";

const firstEvents: string[] = [];
const secondEvents: string[] = [];
let targetId = -1;

function createTrackingHook(events: string[]) {
  return createHook({
    init(asyncId, type) {
      if (type === "ParityMultipleHooks") {
        targetId = asyncId;
        events.push("init");
      }
    },
    before(asyncId) {
      if (asyncId === targetId) events.push("before");
    },
    after(asyncId) {
      if (asyncId === targetId) events.push("after");
    },
  }).enable();
}

const first = createTrackingHook(firstEvents);
const second = createTrackingHook(secondEvents);
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
