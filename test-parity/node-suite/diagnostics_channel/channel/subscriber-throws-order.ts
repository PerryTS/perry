import { channel } from "node:diagnostics_channel";

const ch = channel("dc-subscriber-throws-order-extra");
const events: string[] = [];
process.once("uncaughtException", (err: any) => { events.push("uncaught:" + err.message); });
ch.subscribe(() => { events.push("first"); throw new Error("boom"); });
ch.subscribe(() => { events.push("second"); });
try { ch.publish({}); console.log("publish no throw"); } catch (err: any) { console.log("caught:", err.message); }
await new Promise(resolve => setImmediate(resolve));
console.log("events:", events.join(","));
