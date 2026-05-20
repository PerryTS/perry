import * as os from "node:os";

const total = os.totalmem();
const free = os.freemem();
console.log("total positive:", Number.isFinite(total) && total > 0);
console.log("free nonnegative:", Number.isFinite(free) && free >= 0);
console.log("free <= total:", free <= total);
