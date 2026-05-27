import { Readable } from "node:stream";

const r = new Readable({ read() {} });
const calls: string[] = [];
const fn = () => calls.push("fn");

r.on("custom", fn);
r.on("custom", () => calls.push("middle"));
r.on("custom", fn);

r.removeListener("custom", fn);
r.emit("custom");

console.log("count:", r.listenerCount("custom"));
console.log("calls:", calls.join(","));
