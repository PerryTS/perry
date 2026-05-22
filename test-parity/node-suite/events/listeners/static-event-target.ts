import { getEventListeners, getMaxListeners, setMaxListeners } from "node:events";

const target = new EventTarget();
function listener() {}
target.addEventListener("x", listener);
console.log("listeners:", getEventListeners(target, "x").length);
console.log("max before:", getMaxListeners(target));
setMaxListeners(7, target);
console.log("max after:", getMaxListeners(target));
target.removeEventListener("x", listener);
console.log("listeners after:", getEventListeners(target, "x").length);
