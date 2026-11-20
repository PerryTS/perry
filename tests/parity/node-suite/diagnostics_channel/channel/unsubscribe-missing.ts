import * as dc from "node:diagnostics_channel";

function listener() {}
console.log("unsubscribe missing:", dc.unsubscribe("dc-missing", listener));
console.log("has missing:", dc.hasSubscribers("dc-missing"));
const ch = dc.channel("dc-unsubscribe-missing");
console.log("unsubscribe before subscribe:", ch.unsubscribe(listener));
console.log("has before subscribe:", ch.hasSubscribers);
