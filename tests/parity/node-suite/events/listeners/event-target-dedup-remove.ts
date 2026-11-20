import { getEventListeners } from "node:events";

const target = new EventTarget();
function listener() {}

target.addEventListener("x", listener);
target.addEventListener("x", listener);
console.log("dedup:", getEventListeners(target, "x").length);
target.removeEventListener("x", listener);
console.log("removed:", getEventListeners(target, "x").length);
