import { Readable } from "node:stream";
// setMaxListeners(N) — value sticks across operations.
const r = new Readable({ read() {} });
r.setMaxListeners(50);
r.on("data", () => {});
r.on("end", () => {});
console.log("after ops:", r.getMaxListeners());
console.log("still 50:", r.getMaxListeners() === 50);
