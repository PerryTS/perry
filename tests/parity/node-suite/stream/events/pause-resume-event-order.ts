import { Readable } from "node:stream";
// Multiple pause()/resume() cycles fire the events in cycle order.
const r = new Readable({ read() {} });
const fires: string[] = [];
r.on("pause", () => fires.push("P"));
r.on("resume", () => fires.push("R"));
r.on("data", () => {});
r.pause();
r.resume();
r.pause();
r.resume();
console.log("cycle:", fires.join(""));
