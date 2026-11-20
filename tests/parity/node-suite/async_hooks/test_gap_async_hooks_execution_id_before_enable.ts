import * as ah from "node:async_hooks";
// Before any hook is enabled node still tracks ids for the sync main script
// and for the ticks and immediates it creates. A promise is tracked only while
// a promise hook observes it, so its reaction runs in the enclosing context
// (0 at the top of an ES module, which this entry is: its package.json says
// "type": "module").
console.log("top", ah.executionAsyncId(), ah.triggerAsyncId());
process.nextTick(() => console.log("tick", ah.executionAsyncId() > 1, ah.triggerAsyncId()));
setImmediate(() => console.log("immediate", ah.executionAsyncId() > 1));
Promise.resolve().then(() => console.log("promise", ah.executionAsyncId(), ah.triggerAsyncId()));
