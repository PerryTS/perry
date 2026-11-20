import { EventEmitter } from "node:events";

const ee = new EventEmitter({ captureRejections: true });
const events: string[] = [];
ee.on("x", async () => { throw new Error("async bad"); });
ee.on("error", (err: any) => events.push("error:" + err.message));
ee.emit("x");
await new Promise(resolve => setImmediate(resolve));
console.log("events:", events.join(","));
