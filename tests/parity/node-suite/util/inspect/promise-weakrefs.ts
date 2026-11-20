import { inspect } from "node:util";

console.log("promise pending:", inspect(new Promise(() => {})));
console.log("weakref:", inspect(new WeakRef({ a: 1 })));
console.log("finalization registry:", inspect(new FinalizationRegistry(() => {})));
