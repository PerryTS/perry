import { Readable } from "node:stream";
// setMaxListeners(n) raises the per-event cap. After setting to 20, we
// can attach 15 listeners without warning. getMaxListeners reports the
// current cap.
const r = new Readable({ read() {} });
r.setMaxListeners(20);
console.log("max:", r.getMaxListeners());
for (let i = 0; i < 15; i++) r.on("data", () => {});
console.log("count after 15:", r.listenerCount("data"));
