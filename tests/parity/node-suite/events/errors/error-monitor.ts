import { EventEmitter, errorMonitor } from "node:events";

const ee = new EventEmitter();
const seen: string[] = [];
ee.on(errorMonitor, (err: any) => seen.push("monitor:" + err.message));
try { ee.emit("error", new Error("boom")); } catch (err: any) { seen.push("caught:" + err.message); }
console.log("seen:", seen.join(","));

const handled = new EventEmitter();
const handledSeen: string[] = [];
handled.on(errorMonitor, (err: any) => handledSeen.push("monitor:" + err.message));
handled.on("error", (err: any) => handledSeen.push("handler:" + err.message));
const returned = handled.emit("error", new Error("handled"));
console.log("handled:", handledSeen.join(","), returned);
