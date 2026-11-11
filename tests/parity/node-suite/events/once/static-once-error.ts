import { EventEmitter, once } from "node:events";

const ee = new EventEmitter();
const p = once(ee, "ready").then(() => "resolved", (err: any) => err.message);
console.log("emit return:", ee.emit("error", new Error("bad")));
console.log("ready once:", await p);

const ee2 = new EventEmitter();
const p2 = once(ee2, "error");
ee2.emit("error", new Error("boom"));
const args = await p2;
console.log("error once:", args[0].message);
