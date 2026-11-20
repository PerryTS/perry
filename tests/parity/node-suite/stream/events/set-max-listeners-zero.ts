import { Readable } from "node:stream";
// setMaxListeners(0) — 0 means unlimited; getMaxListeners returns 0.
const r = new Readable({ read() {} });
r.setMaxListeners(0);
console.log("max:", r.getMaxListeners());
console.log("is 0 (unlimited):", r.getMaxListeners() === 0);
// Attach 30 listeners — no warning
for (let i = 0; i < 30; i++) r.on("data", () => {});
console.log("count:", r.listenerCount("data"));
