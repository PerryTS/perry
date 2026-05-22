import { tracingChannel } from "node:diagnostics_channel";
const trace = tracingChannel("dc-trace-unsubscribe-missing");
const handlers = { start() {} };
console.log("missing:", trace.unsubscribe(handlers));
trace.subscribe(handlers);
console.log("present:", trace.unsubscribe(handlers));
console.log("after:", trace.hasSubscribers);
