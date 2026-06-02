import { availableParallelism, endianness, machine, loadavg, version, devNull } from "node:os";

console.log("available positive:", availableParallelism() > 0);
console.log("endianness known:", ["LE", "BE"].includes(endianness()));
console.log("machine string:", typeof machine() === "string");
console.log("load length:", loadavg().length);
console.log("version string:", typeof version() === "string");
console.log("devNull string:", typeof devNull === "string");
