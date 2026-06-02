import { EventEmitter } from "node:events";

const em = new EventEmitter();
let count = 0;
em.on("tick", () => { count++; });
console.log("emit returned:", em.emit("tick"));
console.log("count:", count);
