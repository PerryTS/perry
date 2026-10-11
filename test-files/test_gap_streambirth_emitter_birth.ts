import { EventEmitter } from "node:events";
function layout(e: any) {
  console.log(JSON.stringify(Object.keys(e)), JSON.stringify(Object.getOwnPropertyNames(e)),
    Object.getPrototypeOf(e._events) === null, e._eventsCount, e.getMaxListeners());
}
layout(new EventEmitter());
layout(new EventEmitter({ captureRejections: true }));
class E extends EventEmitter { constructor(o?: any) { super(o); } }
layout(new E());
const first: any = new EventEmitter();
const second: any = new EventEmitter();
console.log("separate", first._events !== second._events);
const log: string[] = [];
first.once("x", (n: number) => log.push("once:" + n));
first.on("x", (n: number) => log.push("on:" + n));
first.emit("x", 1); first.emit("x", 2);
console.log(log.join("|"), first._eventsCount, second._eventsCount);
first.setMaxListeners(4); console.log(first.getMaxListeners(), second.getMaxListeners());
const held: any[] = [];
for (let i = 0; i < 128; i++) {
  const e: any = new EventEmitter();
  e.on("x", (n: number) => { e.total = n + i; });
  held.push(e);
}
let sum = 0;
for (let i = 0; i < held.length; i++) {
  const e = held[i];
  e.emit("x", i);
  sum += e.total;
}
console.log("retained", held.length, sum, held[0]._events !== held[127]._events);
