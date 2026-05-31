import { EventEmitter } from "node:events";

const em = new EventEmitter();
function h(this: any, ...args: any[]) {
  console.log("handler:", args.join(","), this === em);
}
em.once("x", h);
const raw = em.rawListeners("x");
console.log("listeners count:", em.listeners("x").length);
console.log("raw count:", raw.length);
console.log("same handler:", raw[0] === h);
console.log("wrapper listener:", raw[0].listener === h);
raw[0]("a", "b");
console.log("after raw call:", em.listenerCount("x"));
