import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-trace-has");
function start() {}
function asyncEnd() {}
console.log("initial:", ch.hasSubscribers);
ch.subscribe({ start });
console.log("after subscribe object:", ch.hasSubscribers, ch.start.hasSubscribers);
console.log("unsubscribe object:", ch.unsubscribe({ start }));
console.log("after unsubscribe object:", ch.hasSubscribers);
ch.asyncEnd.subscribe(asyncEnd);
console.log("after direct asyncEnd:", ch.hasSubscribers, ch.asyncEnd.hasSubscribers);
console.log("direct asyncEnd unsubscribe:", ch.asyncEnd.unsubscribe(asyncEnd));
console.log("final:", ch.hasSubscribers);
