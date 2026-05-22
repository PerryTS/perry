import { channel } from "node:diagnostics_channel";
const ch = channel("dc-method-return-values");
function listener() {}
console.log("subscribe:", ch.subscribe(listener));
console.log("publish:", ch.publish({}));
console.log("unsubscribe:", ch.unsubscribe(listener));
