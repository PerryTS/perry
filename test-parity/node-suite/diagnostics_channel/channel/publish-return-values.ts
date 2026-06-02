import { channel } from "node:diagnostics_channel";

const ch = channel("dc-publish-return-values");
console.log("publish no subs:", ch.publish({ a: 1 }));
function sub(msg: any) { console.log("sub msg:", msg.a); }
ch.subscribe(sub);
console.log("publish one sub:", ch.publish({ a: 2 }));
ch.unsubscribe(sub);
console.log("publish after unsub:", ch.publish({ a: 3 }));
