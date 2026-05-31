import * as events from "node:events";

console.log("defaultMaxListeners:", events.defaultMaxListeners);
console.log("usingDomains:", events.usingDomains);
console.log("EventEmitter usingDomains:", events.EventEmitter.usingDomains);
console.log("init type:", typeof events.init);
console.log("init identity:", events.init === events.EventEmitter.init);
console.log("errorMonitor:", typeof events.errorMonitor, String(events.errorMonitor));
console.log("captureRejections:", events.captureRejections);
console.log("captureRejectionSymbol:", typeof events.captureRejectionSymbol, String(events.captureRejectionSymbol));
console.log("keys include usingDomains:", Object.keys(events).includes("usingDomains"));
console.log("keys include init:", Object.keys(events).includes("init"));
