// #10952 / #10926: once `new AsyncResource(...)` returns an ordinary handle
// OBJECT instead of the raw native `Box`, every native caller that stored
// `js_async_resource_new`'s result and then branded it by BACKING-registry
// membership stopped matching:
//   * `set_async_resource_event_emitter` silently dropped the link, so
//     `eear.asyncResource.eventEmitter` was `undefined` (direct path);
//   * the EventEmitterAsyncResource subclass brand check in
//     `node_stream_dispatch` rejected its own hidden resource, so `emit` threw
//     "Cannot read private member ..." (subclass path).
// The stdlib emitter also cached the (now movable) resource object in a slot
// no GC scanner visited; the churn below crosses collections before re-reading.
import { EventEmitterAsyncResource } from "node:events";
import { executionAsyncId, AsyncResource } from "node:async_hooks";

const e: any = new EventEmitterAsyncResource({ name: "PROBE" });
const id = e.asyncId;
console.log("id-positive", typeof id === "number" && id > 0);
console.log("trigger-number", typeof e.triggerAsyncId);
const r: any = e.asyncResource;
console.log("resource-is-AR", r instanceof AsyncResource);
console.log("resource-asyncId-matches", r.asyncId() === id);
console.log("resource-emitter", r.eventEmitter === e);
let inside = -1;
e.on("x", () => { inside = executionAsyncId(); });
e.emit("x");
console.log("listener-in-scope", inside === id);

class Sub extends EventEmitterAsyncResource { constructor() { super({ name: "SUB" }); } }
const s: any = new Sub();
const sid = s.asyncId;
console.log("sub-id-positive", typeof sid === "number" && sid > 0);
let sinside = -1;
s.on("y", () => { sinside = executionAsyncId(); });
s.emit("y");
console.log("sub-listener-in-scope", sinside === sid);
console.log("sub-resource-emitter", s.asyncResource.eventEmitter === s);

// churn: the emitter's resource must survive collections
const junk: any[] = [];
for (let i = 0; i < 200000; i++) { junk.push({ i, s: "x" + i }); if (junk.length > 1000) junk.length = 0; }
if (typeof (globalThis as any).gc === "function") (globalThis as any).gc();
console.log("after-gc-id", e.asyncId === id, s.asyncId === sid);
inside = -1; e.emit("x");
console.log("after-gc-listener-in-scope", inside === id);
sinside = -1; s.emit("y");
console.log("after-gc-sub-listener-in-scope", sinside === sid);
console.log("after-gc-resource-emitter", e.asyncResource.eventEmitter === e, s.asyncResource.eventEmitter === s);
e.emitDestroy();
console.log("done");
