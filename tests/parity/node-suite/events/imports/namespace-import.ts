import * as events from "node:events";

console.log("EventEmitter type:", typeof events.EventEmitter);
console.log("namespace keys count positive:", Object.keys(events).length > 0);
