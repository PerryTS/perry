import { channel } from "node:diagnostics_channel";

const ch = channel("dc-subscriber-errors");
const events: string[] = [];
const error = new Error("subscriber boom");
process.once("uncaughtException", (err: Error) => {
  console.log("uncaught same error:", err === error);
  console.log("events:", events.join(","));
});
ch.subscribe(() => {
  events.push("thrower");
  throw error;
});
ch.subscribe(() => {
  events.push("after");
});
try {
  ch.publish({ ok: true });
  console.log("publish returned");
} catch (err: any) {
  console.log("publish threw:", err?.message);
}
