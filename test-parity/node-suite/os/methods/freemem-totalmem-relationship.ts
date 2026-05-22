import os from "node:os";

const free = os.freemem();
const total = os.totalmem();
console.log("types:", typeof free, typeof total);
console.log("nonnegative:", free >= 0, total >= 0);
console.log("free<=total:", free <= total);
