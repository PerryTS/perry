// #11258: the EventEmitterAsyncResource native backing caches a subclass
// emitter's `this` as a raw address. With no GC root scanner visiting it, a
// collection that moved the emitter left the cache pointing at the stale
// from-space copy, so `sub.asyncResource.eventEmitter === sub` became false.
// Allocation churn below crosses (moving) collections before re-reading.
import { EventEmitterAsyncResource } from "node:events";

class Sub extends EventEmitterAsyncResource {
  constructor(name: string) { super({ name }); }
}

const subs: any[] = [];
for (let i = 0; i < 8; i++) subs.push(new Sub("SUB" + i));
console.log("before", subs.every((s) => s.asyncResource.eventEmitter === s));

const junk: any[] = [];
for (let round = 0; round < 4; round++) {
  for (let i = 0; i < 100000; i++) {
    junk.push({ i, s: "x" + i });
    if (junk.length > 1000) junk.length = 0;
  }
  if (typeof (globalThis as any).gc === "function") (globalThis as any).gc();
  console.log("round", round, subs.every((s) => s.asyncResource.eventEmitter === s));
}

// A resource read out and kept while its emitter is still referenced keeps
// pointing at the live emitter too.
const r: any = subs[3].asyncResource;
for (let i = 0; i < 100000; i++) junk.push({ i });
console.log("detached-resource", r.eventEmitter === subs[3]);
let hits = 0;
r.eventEmitter.on("ping", () => { hits++; });
subs[3].emit("ping");
console.log("listener-via-resource", hits);
