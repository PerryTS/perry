import { WritableStream, CountQueuingStrategy } from "node:stream/web";
// WritableStream(undefined, strategy) — uses default sink with custom strategy.
const strategy = new CountQueuingStrategy({ highWaterMark: 7 });
const ws = new WritableStream(undefined, strategy);
console.log("locked:", ws.locked);
console.log("constructed:", ws instanceof WritableStream);
