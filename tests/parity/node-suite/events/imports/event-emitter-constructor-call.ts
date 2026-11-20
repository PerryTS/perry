import { EventEmitter } from "node:events";

const receiver = { label: "counter" };
const result = EventEmitter.call(receiver);

console.log("result:", result === undefined);
console.log("own keys:", Object.keys(receiver).join(","));
console.log("events type:", typeof receiver._events);
console.log("events count:", receiver._eventsCount);
console.log("max listeners:", receiver._maxListeners === undefined);
