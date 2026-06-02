import dcDefault, * as dc from "node:diagnostics_channel";

console.log("namespace missing typeof:", typeof (dc as any).notARealExport);
console.log("default missing typeof:", typeof (dcDefault as any).notARealExport);
