import { EventEmitter } from "node:events";

const em = new EventEmitter();
const obj: any = { toString: () => "OBJ" };
const obj2: any = { toString: () => "OBJ" };
const sym = Symbol("evt");

em.on(123 as any, () => console.log("number fired"));
em.on(null as any, () => console.log("null fired"));
em.on(undefined as any, () => console.log("undefined fired"));
em.on(obj, () => console.log("object fired"));
em.on(sym, () => console.log("symbol fired"));

console.log("names:", em.eventNames().map((name) => `${typeof name}:${String(name)}`).join("|"));
console.log("count number:", em.listenerCount("123"));
console.log("count null:", em.listenerCount("null"));
console.log("count undefined:", em.listenerCount("undefined"));
console.log("count object:", em.listenerCount(obj2));
console.log("symbol identity:", em.eventNames().includes(sym));
em.emit(123 as any);
em.emit(null as any);
em.emit(undefined as any);
em.emit(obj2);
em.emit(sym);
