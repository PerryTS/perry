import { EventEmitter } from "node:events";

const em = new EventEmitter();
em.on("symbolic", () => {});
console.log("count:", em.listenerCount("symbolic"));
console.log("name:", em.eventNames()[0]);
