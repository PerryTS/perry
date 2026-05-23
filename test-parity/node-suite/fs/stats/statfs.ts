import * as fs from "node:fs";

const stats = fs.statfsSync("/tmp");
console.log("statfs bsize positive:", stats.bsize > 0);
console.log("statfs blocks positive:", stats.blocks > 0);
console.log("statfs bfree number:", typeof stats.bfree);
console.log("statfs bavail number:", typeof stats.bavail);
