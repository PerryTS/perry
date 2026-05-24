import { Readable } from "node:stream";
// Removing a listener during emit() — the listener still fires this round
// (because emit() snapshots the listener array).
const r = new Readable({ read() {} });
const fired: string[] = [];
const fn1 = () => {
  fired.push("fn1");
  r.removeListener("custom", fn2);
};
const fn2 = () => {
  fired.push("fn2");
};
r.on("custom", fn1);
r.on("custom", fn2);
r.emit("custom");
console.log("fired:", fired.join(","));
