import dcDefault, * as dc from "node:diagnostics_channel";

console.log("default channel:", typeof (dcDefault as any).channel);
console.log("same channel fn:", (dcDefault as any).channel === dc.channel);
console.log("same object channel:", dc.channel("identity") === (dcDefault as any).channel("identity"));
