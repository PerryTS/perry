import { Readable } from "node:stream";
// 'removeListener' meta-event fires AFTER the listener is removed.
const r = new Readable({ read() {} });
const target = () => {};
r.on("custom", target);
let countAtMeta: number = -1;
r.on("removeListener", (event) => {
  if (event === "custom") countAtMeta = r.listenerCount("custom");
});
r.removeListener("custom", target);
console.log("count at meta:", countAtMeta);
console.log("count after:", r.listenerCount("custom"));
