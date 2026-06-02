import * as dc from "node:diagnostics_channel";
function listener() {}
console.log("subscribe:", dc.subscribe("dc-module-return-values", listener));
console.log("publish:", dc.channel("dc-module-return-values").publish("x"));
console.log("unsubscribe:", dc.unsubscribe("dc-module-return-values", listener));
