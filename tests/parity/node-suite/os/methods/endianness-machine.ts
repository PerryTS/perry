import * as os from "node:os";

console.log("endianness known:", ["LE", "BE"].includes(os.endianness()));
console.log("machine string:", typeof os.machine() === "string");
console.log("machine nonempty:", os.machine().length > 0);
