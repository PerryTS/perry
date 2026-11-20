import { Readable } from "node:stream";
// autoDestroy defaults to true — after end, the stream destroys itself
// (firing 'close' even without explicit destroy()).
const r = new Readable({ read() {} });
let closedFired = false;
r.on("close", () => (closedFired = true));
r.on("data", () => {});
r.on("end", () => {
  setImmediate(() => {
    console.log("destroyed:", r.destroyed);
    console.log("closed fired:", closedFired);
  });
});
r.push("a");
r.push(null);
