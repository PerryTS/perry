import { config } from "./cyc_lib.ts";
// Top-level read of the importer's binding: legal in Node because this module
// only evaluates after cyc_lib.ts has finished.
console.log("cyc_z init", typeof config);
export const K = config.n * 10;
