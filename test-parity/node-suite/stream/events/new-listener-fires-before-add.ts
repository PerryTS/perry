import { Readable } from "node:stream";
// 'newListener' fires BEFORE the listener is added to the list.
const r = new Readable({ read() {} });
r.on("newListener", (event) => {
  if (event === "custom") {
    console.log("count at newListener:", r.listenerCount("custom"));
    // Should still be 0 (the listener hasn't been added yet)
  }
});
r.on("custom", () => {});
console.log("count after add:", r.listenerCount("custom"));
