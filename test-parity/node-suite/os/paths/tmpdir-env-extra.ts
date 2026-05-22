import os from "node:os";

console.log("tmpdir type:", typeof os.tmpdir(), os.tmpdir().length > 0);
console.log("homedir type:", typeof os.homedir());
console.log("devNull:", os.devNull.includes("null") || os.devNull.includes("NUL"));
