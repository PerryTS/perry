import * as events from "node:events";

console.log("defaultMaxListeners:", events.defaultMaxListeners);
console.log("errorMonitor:", typeof events.errorMonitor, String(events.errorMonitor));
console.log("captureRejections:", events.captureRejections);
console.log("captureRejectionSymbol:", typeof events.captureRejectionSymbol, String(events.captureRejectionSymbol));
