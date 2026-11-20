import * as os from "node:os";

const n = os.availableParallelism();
console.log("available number:", Number.isInteger(n));
console.log("available positive:", n > 0);
