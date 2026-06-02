import { tracingChannel } from "node:diagnostics_channel";
const trace = tracingChannel("dc-trace-end-only");
let ends = 0;
trace.subscribe({ end: () => { ends++; } });
console.log("has:", trace.hasSubscribers, trace.end.hasSubscribers, trace.start.hasSubscribers);
trace.traceSync(() => "ok", {});
console.log("ends:", ends);
