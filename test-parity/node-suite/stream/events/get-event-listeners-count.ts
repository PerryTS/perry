import { Readable } from "node:stream";
import { getEventListeners } from "node:events";
// getEventListeners — returns array with all listeners for the event.
const r = new Readable({ read() {} });
r.on("custom", () => {});
r.on("custom", () => {});
r.on("custom", () => {});
const arr = getEventListeners(r, "custom");
console.log("isArray:", Array.isArray(arr));
console.log("length:", arr.length);
