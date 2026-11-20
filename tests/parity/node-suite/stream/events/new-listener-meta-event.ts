import { Readable } from "node:stream";
// 'newListener' is fired when a listener is added, BEFORE it joins the
// listener list. Useful for hooking into setup.
const r = new Readable({ read() {} });
const seenEvents: string[] = [];
r.on("newListener", (event) => seenEvents.push(event));
r.on("data", () => {});
r.on("end", () => {});
console.log("newListener fired for:", seenEvents.join(","));
