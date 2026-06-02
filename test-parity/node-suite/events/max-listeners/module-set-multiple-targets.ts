import { EventEmitter, getMaxListeners, setMaxListeners } from "node:events";

const a = new EventEmitter();
const b = new EventEmitter();
setMaxListeners(6, a, b);
console.log("a:", getMaxListeners(a));
console.log("b:", getMaxListeners(b));
