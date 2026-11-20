import { Readable } from "node:stream";
// Adding a listener INSIDE emit() — the new listener is NOT invoked this round.
const r = new Readable({ read() {} });
const order: string[] = [];
const newListener = () => order.push("late");
r.on("custom", () => {
  order.push("first");
  r.on("custom", newListener); // added during emit
});
r.emit("custom"); // should only fire 'first'
console.log("first emit:", order.join(","));
r.emit("custom"); // now 'late' fires
console.log("second emit:", order.join(","));
