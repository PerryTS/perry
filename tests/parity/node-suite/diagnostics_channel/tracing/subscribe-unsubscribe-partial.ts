import { tracingChannel } from "node:diagnostics_channel";

const trace = tracingChannel("dc-trace-partial");
let starts = 0;
const handlers = { start() { starts++; } };
console.log("initial:", trace.hasSubscribers);
trace.subscribe(handlers);
console.log("after subscribe:", trace.hasSubscribers, trace.start.hasSubscribers, trace.end.hasSubscribers);
trace.traceSync(() => "ok", { name: "ctx" });
console.log("starts:", starts);
console.log("unsubscribe:", trace.unsubscribe(handlers));
console.log("after unsubscribe:", trace.hasSubscribers, trace.start.hasSubscribers);
