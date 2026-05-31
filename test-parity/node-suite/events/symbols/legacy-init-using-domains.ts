import * as events from "node:events";
import { EventEmitter } from "node:events";

const plain: any = {};
const result = events.init.call(plain);

console.log("usingDomains:", events.usingDomains, EventEmitter.usingDomains);
console.log("init type:", typeof events.init, events.init === EventEmitter.init, events.init.length);
console.log("init result:", result === undefined);
console.log("init keys:", Object.keys(plain).join(","));
console.log("events proto null:", Object.getPrototypeOf(plain._events) === null);
console.log("events count:", plain._eventsCount);
console.log("max listeners own:", Object.prototype.hasOwnProperty.call(plain, "_maxListeners"), plain._maxListeners);
