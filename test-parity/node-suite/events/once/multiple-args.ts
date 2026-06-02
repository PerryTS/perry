import { EventEmitter, once } from "node:events";

const ee = new EventEmitter();
const p = once(ee, "data");
ee.emit("data", "a", 2, true);
const args = await p;
console.log("args:", args.map(String).join(","));
