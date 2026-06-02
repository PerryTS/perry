import { Readable } from "node:stream";
// 'removeListener' is emitted when a listener is removed via removeListener().
const r = new Readable({ read() {} });
let fired = false;
let firedEvent: string | null = null;
r.on("removeListener", (event) => {
  fired = true;
  firedEvent = event;
});
const fn = () => {};
r.on("data", fn);
r.removeListener("data", fn);
console.log("removeListener event fired:", fired);
console.log("event name:", firedEvent);
