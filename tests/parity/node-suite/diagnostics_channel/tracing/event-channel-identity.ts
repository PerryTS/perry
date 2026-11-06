import { channel, tracingChannel } from "node:diagnostics_channel";
const trace = tracingChannel("dc-trace-event-identity");
console.log("start identity:", trace.start === channel("tracing:dc-trace-event-identity:start"));
console.log("end identity:", trace.end === channel("tracing:dc-trace-event-identity:end"));
console.log("error identity:", trace.error === channel("tracing:dc-trace-event-identity:error"));
