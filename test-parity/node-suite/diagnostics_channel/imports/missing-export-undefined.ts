import dcDefault, * as dc from "node:diagnostics_channel";

console.log("namespace boundedChannel typeof:", typeof (dc as any).boundedChannel);
console.log("default boundedChannel typeof:", typeof (dcDefault as any).boundedChannel);
