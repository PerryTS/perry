import { EventEmitter, errorMonitor } from "node:events";

const ee = new EventEmitter();
const seen: string[] = [];
ee.on(errorMonitor, (err: any) => seen.push("monitor:" + err.message));
try { ee.emit("error", new Error("boom")); } catch (err: any) { seen.push("caught:" + err.message); }
console.log("seen:", seen.join(","));
