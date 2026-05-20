import { EventEmitter } from "node:events";

const em = new EventEmitter();
const a = em.on("a", () => {});
const b = em.addListener("b", () => {});
console.log("on chain:", a === em);
console.log("addListener chain:", b === em);
console.log("names:", em.eventNames());
