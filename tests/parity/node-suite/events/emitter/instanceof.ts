import { EventEmitter } from "node:events";

const em = new EventEmitter();
console.log("instanceof:", em instanceof EventEmitter);
